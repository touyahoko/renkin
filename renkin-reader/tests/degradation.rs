//! 劣化検証: charm-pics の画像に撮影で起こりそうな劣化を段階的に加えて読み、自信ありの読み間違い (すり抜け) が
//! 出ないかを確かめる。劣化の種類と強さは experimental-charm-reader の robustness.LEVELS と同じ。
//! 加え方も同じ考え方だが、OpenCV と画素まで同じにはしていない (ぼかしは箱フィルタ 3 回で近似、ノイズの乱数、
//! JPEG のエンコーダが違う)。読めなくなる境目と、すり抜けが無いかを見るためのもの。
//!
//! 劣化なしで期待どおりに読めた画像だけを使い、(画像, 劣化, 強さ) ごとに
//!   OK / 撮り直し / すり抜け (自信ありで中身が違う) / 何個目かのすり抜け (自信ありで何個目かが違う)
//! を数える。どちらかのすり抜けがあれば失敗にする。
//!
//! とても重い (画像 × 37 段階を読む。3 スレッドで 30 分前後) ので
//! `cargo test --release --test degradation -- --ignored --nocapture` で流す。charm-pics が無ければ何もせずに通る。
//! 環境変数: RENKIN_KINDS (劣化の種類をカンマ区切りで絞る。例: ピンボケ,縮小)、RENKIN_LIMIT (先頭から何枚だけ使うか。動作確認用)、
//! RENKIN_ONLY (名前にこれを含む画像だけ。カンマ区切りで複数)、RENKIN_EXACT_BLUR (ピンボケを箱フィルタの近似ではなく、
//! cv2.GaussianBlur と同じ大きさの核のガウスぼかしにする。遅い)、RENKIN_DUMP (すり抜けたときに、劣化を加えた写真
//! (長い辺 1600px に縮めたもの) と位置合わせ後の画面を、このディレクトリに PNG で書き出す)、RENKIN_THREADS (スレッド数。既定は 3)

#[path = "support/e2e.rs"]
mod e2e;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use e2e::Outcome;
use renkin_reader::{ReadResult, Reader};

/// (劣化, 強さの一覧)。劣化なしの強さ (縮小 1.0、ガンマ 1.0、JPEG 100、ほかは 0) は、劣化前の読み取りと同じなので省く
const LEVELS: &[(&str, &[f64])] = &[
    ("縮小", &[0.5, 0.35, 0.25, 0.18, 0.14, 0.11, 0.09]), // 写真の縮小率
    ("ピンボケ", &[1.0, 2.0, 3.0, 4.0, 6.0]),             // σ (画面 1px あたり)
    ("手ぶれ", &[2.0, 4.0, 8.0, 12.0, 16.0]),             // ぶれ幅 (画面 px)
    ("ノイズ", &[10.0, 20.0, 30.0, 45.0, 60.0]),          // 正規分布の σ (8bit)
    ("JPEG", &[50.0, 30.0, 15.0, 8.0, 4.0]),              // 品質
    ("ガンマ", &[0.4, 0.6, 1.6, 2.2, 3.0]),               // <1 明るく, >1 暗く
    ("反射", &[60.0, 100.0, 140.0, 180.0, 220.0]),        // にじみの明るさ (8bit)
];

/// RGBA の画像
struct Img {
    px: Vec<u8>,
    w: usize,
    h: usize,
}

/// 劣化を加える。ppx: 画面 1px あたりの写真 px。center: パネルの中心 (写真の座標)
fn degrade(img: &Img, kind: &str, level: f64, ppx: f64, center: [f32; 2]) -> Img {
    match kind {
        "縮小" => resize_area(img, level),
        "ピンボケ" if std::env::var("RENKIN_EXACT_BLUR").is_ok() => {
            gaussian_exact(img, level * ppx)
        }
        "ピンボケ" => gaussian_like(img, level * ppx),
        "手ぶれ" => {
            let n = ((level * ppx) as usize | 1).max(3);
            let mut out = Img {
                px: img.px.clone(),
                w: img.w,
                h: img.h,
            };
            box_rows(&mut out, n);
            out
        }
        "ノイズ" => {
            // 正規乱数を毎回作ると遅いので、表から引く (表の長さは画素の並びと揃わない素数にして、模様が出ないようにする)
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
            let table: Vec<f64> = (0..NOISE_TABLE).map(|_| rng.normal() * level).collect();
            let mut i = 0;
            map_rgb(img, |_, _, v| {
                i = (i + 1) % NOISE_TABLE;
                v + table[i]
            })
        }
        "JPEG" => jpeg(img, level as u8),
        "ガンマ" => {
            let lut: Vec<f64> = (0..256)
                .map(|v| (v as f64 / 255.0).powf(level) * 255.0)
                .collect();
            map_rgb(img, |_, _, v| lut[v as usize])
        }
        "反射" => {
            let r = 90.0 * ppx; // 画面で半径 90px ほどのにじみ (パネルの高さの半分弱)
            let (cx, cy) = (center[0] as f64, center[1] as f64);
            map_rgb(img, |x, y, v| {
                let d2 = (x as f64 - cx).powi(2) + (y as f64 - cy).powi(2);
                v + level * (-d2 / (2.0 * r * r)).exp()
            })
        }
        _ => unreachable!("{kind}"),
    }
}

/// ノイズの表の長さ (素数)
const NOISE_TABLE: usize = 1_000_003;

/// RGB の各画素を f(x, y, 値) に置き換える (0〜255 に丸める。透明度はそのまま)
fn map_rgb(img: &Img, mut f: impl FnMut(usize, usize, f64) -> f64) -> Img {
    let mut px = img.px.clone();
    for y in 0..img.h {
        for x in 0..img.w {
            let i = (y * img.w + x) * 4;
            for c in 0..3 {
                px[i + c] = f(x, y, px[i + c] as f64).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Img {
        px,
        w: img.w,
        h: img.h,
    }
}

/// 面積平均で縮小する (cv2.INTER_AREA と同じ考え方)
fn resize_area(img: &Img, scale: f64) -> Img {
    let (ow, oh) = (
        ((img.w as f64) * scale).round() as usize,
        ((img.h as f64) * scale).round() as usize,
    );
    // 1 次元の重み: 出力の各画素が、入力のどの画素をどれだけ含むか
    let weights = |n_in: usize, n_out: usize| -> Vec<Vec<(usize, f64)>> {
        let s = n_in as f64 / n_out as f64;
        (0..n_out)
            .map(|o| {
                let (a, b) = (o as f64 * s, (o + 1) as f64 * s);
                (a.floor() as usize..(b.ceil() as usize).min(n_in))
                    .map(|i| (i, ((i + 1) as f64).min(b) - (i as f64).max(a)))
                    .filter(|&(_, w)| w > 0.0)
                    .map(|(i, w)| (i, w / s))
                    .collect()
            })
            .collect()
    };
    let (wx, wy) = (weights(img.w, ow), weights(img.h, oh));
    // 横を縮めてから縦を縮める
    let mut tmp = vec![0f64; ow * img.h * 3];
    for y in 0..img.h {
        for (ox, ws) in wx.iter().enumerate() {
            for c in 0..3 {
                tmp[(y * ow + ox) * 3 + c] = ws
                    .iter()
                    .map(|&(x, w)| img.px[(y * img.w + x) * 4 + c] as f64 * w)
                    .sum();
            }
        }
    }
    let mut px = vec![255u8; ow * oh * 4];
    for (oy, ws) in wy.iter().enumerate() {
        for ox in 0..ow {
            for c in 0..3 {
                let v: f64 = ws
                    .iter()
                    .map(|&(y, w)| tmp[(y * ow + ox) * 3 + c] * w)
                    .sum();
                px[(oy * ow + ox) * 4 + c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Img { px, w: ow, h: oh }
}

/// 横方向に幅 n (奇数) の平均をとる。端は端の画素を伸ばす
fn box_rows(img: &mut Img, n: usize) {
    let r = n / 2;
    let w = img.w;
    // 端を伸ばした 1 行 (前後に r 画素ずつ足す) を作り、整数の累積和で平均をとる
    let mut padded = vec![0u32; w + 2 * r];
    for y in 0..img.h {
        let row = &mut img.px[y * w * 4..(y + 1) * w * 4];
        for c in 0..3 {
            for (i, p) in padded.iter_mut().enumerate() {
                *p = row[i.saturating_sub(r).min(w - 1) * 4 + c] as u32;
            }
            let mut sum: u32 = padded[..n].iter().sum();
            for x in 0..w {
                row[x * 4 + c] = ((sum + n as u32 / 2) / n as u32) as u8;
                if x + n < padded.len() {
                    sum = sum + padded[x + n] - padded[x];
                }
            }
        }
    }
}

/// 縦横を入れ替える (縦方向の箱フィルタを、横方向のもので済ませるため)
fn transpose(img: &Img) -> Img {
    let mut px = vec![0u8; img.px.len()];
    for y in 0..img.h {
        for x in 0..img.w {
            let (s, d) = ((y * img.w + x) * 4, (x * img.h + y) * 4);
            px[d..d + 4].copy_from_slice(&img.px[s..s + 4]);
        }
    }
    Img {
        px,
        w: img.h,
        h: img.w,
    }
}

/// ガウスぼかしを、箱フィルタ 3 回で近似する (σ が大きくても速い)
fn gaussian_like(img: &Img, sigma: f64) -> Img {
    // 幅 n の箱を 3 回かけた分散は 3 (n² - 1) / 12
    let n = (((4.0 * sigma * sigma + 1.0).sqrt()).round() as usize | 1).max(1);
    let mut out = Img {
        px: img.px.clone(),
        w: img.w,
        h: img.h,
    };
    if n < 3 {
        return out;
    }
    for _ in 0..3 {
        box_rows(&mut out, n);
    }
    let mut t = transpose(&out);
    for _ in 0..3 {
        box_rows(&mut t, n);
    }
    transpose(&t)
}

/// ガウスぼかし (cv2.GaussianBlur(img, (0, 0), sigma) と同じ核の大きさ、端は反射 (BORDER_REFLECT_101))
fn gaussian_exact(img: &Img, sigma: f64) -> Img {
    let n = ((sigma * 6.0 + 1.0).round() as usize) | 1;
    let r = (n / 2) as isize;
    let k: Vec<f64> = (-r..=r)
        .map(|i| (-((i * i) as f64) / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f64 = k.iter().sum();
    let k: Vec<f64> = k.iter().map(|v| v / total).collect();
    let reflect = |i: isize, len: usize| -> usize {
        let len = len as isize;
        if len == 1 {
            return 0;
        }
        let mut i = i;
        while i < 0 || i >= len {
            i = if i < 0 { -i } else { 2 * len - 2 - i };
        }
        i as usize
    };
    // 横、縦の順にかける (途中は f64 のまま)
    let (w, h) = (img.w, img.h);
    let mut tmp = vec![0f64; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                tmp[(y * w + x) * 3 + c] = k
                    .iter()
                    .enumerate()
                    .map(|(j, kv)| {
                        kv * img.px[(y * w + reflect(x as isize + j as isize - r, w)) * 4 + c]
                            as f64
                    })
                    .sum();
            }
        }
    }
    let mut px = img.px.clone();
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let v: f64 = k
                    .iter()
                    .enumerate()
                    .map(|(j, kv)| {
                        kv * tmp[(reflect(y as isize + j as isize - r, h) * w + x) * 3 + c]
                    })
                    .sum();
                px[(y * w + x) * 4 + c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Img { px, w, h }
}

/// JPEG で保存し直す
fn jpeg(img: &Img, quality: u8) -> Img {
    let rgb: Vec<u8> = img
        .px
        .chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality)
        .encode(
            &rgb,
            img.w as u32,
            img.h as u32,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    let rgba = image::load_from_memory(&buf).unwrap().to_rgba8();
    Img {
        px: rgba.into_raw(),
        w: img.w,
        h: img.h,
    }
}

/// ノイズ用の乱数 (xorshift64* と Box-Muller)
struct Rng(u64);

impl Rng {
    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        ((self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    fn normal(&mut self) -> f64 {
        let (u, v) = (self.uniform(), self.uniform());
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
}

/// すり抜けた画像を書き出す: <名前>__<劣化><強さ>_photo.png (長い辺 1600px) と _screen.png (位置合わせ後の画面)
fn dump(dir: &Path, name: &str, kind: &str, level: f64, photo: &Img, screen: Option<&[u8]>) {
    std::fs::create_dir_all(dir).unwrap();
    let base = format!("{}__{kind}{level}", name.replace('/', "__"));
    let small = resize_area(photo, (1600.0 / photo.w.max(photo.h) as f64).min(1.0));
    image::save_buffer(
        dir.join(format!("{base}_photo.png")),
        &small.px,
        small.w as u32,
        small.h as u32,
        image::ExtendedColorType::Rgba8,
    )
    .unwrap();
    if let Some(screen) = screen {
        image::save_buffer(
            dir.join(format!("{base}_screen.png")),
            screen,
            1280,
            720,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    }
}

fn charm_pics() -> PathBuf {
    std::env::var("CHARM_PICS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../charm-pics"))
}

/// HEIC は tests/golden/photos/ に書き出した JPEG を開く (tests/charm_pics.rs と同じ)
fn open(a: &e2e::Answer) -> Img {
    let ext = a.path.extension().unwrap().to_string_lossy().to_lowercase();
    let path = if ext == "heic" || ext == "heif" {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/photos")
            .join(a.name.replace('/', "__") + ".jpg")
    } else {
        a.path.clone()
    };
    let (px, w, h) = e2e::load(&path);
    Img {
        px,
        w: w as usize,
        h: h as usize,
    }
}

/// (画像の名前, 劣化, 強さ, 結果, 何個目かを自信ありで読み違えたか)
type Row = (String, &'static str, f64, Outcome, bool);

#[test]
#[ignore]
fn no_slips_under_degradation() {
    let root = charm_pics();
    if !root.join("tests").is_dir() {
        println!("charm-pics が無いので飛ばす ({})", root.display());
        return;
    }
    let kinds: Option<Vec<String>> = std::env::var("RENKIN_KINDS")
        .ok()
        .map(|k| k.split(',').map(str::to_string).collect());
    let levels: Vec<(&'static str, f64)> = LEVELS
        .iter()
        .filter(|(k, _)| kinds.as_ref().is_none_or(|ks| ks.iter().any(|x| x == k)))
        .flat_map(|&(k, ls)| ls.iter().map(move |&l| (k, l)))
        .collect();
    // 撮り直しが期待の画像は、劣化を加えても意味がないので使わない
    let limit = std::env::var("RENKIN_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let all: Vec<e2e::Answer> = e2e::answers(&root, &["tests", "synthetic"])
        .into_iter()
        .filter(|a| !a.retake)
        .filter(|a| {
            std::env::var("RENKIN_ONLY")
                .map_or(true, |only| only.split(',').any(|o| a.name.contains(o)))
        })
        .take(limit)
        .collect();
    println!("画像 {} 枚 × {} 段階", all.len(), levels.len());

    let next = AtomicUsize::new(0);
    let rows: Mutex<Vec<Row>> = Mutex::new(Vec::new());
    let skipped: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let started = std::time::Instant::now();
    let work = || {
        let mut reader = Reader::new();
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            let Some(a) = all.get(i) else { break };
            let t = std::time::Instant::now();
            let img = open(a);
            // 劣化前に期待どおりに読めて、位置合わせの結果 (画面の大きさとパネルの位置) が分かるものだけを使う
            let clean = reader.read(&img.px, img.w as u32, img.h as u32).unwrap();
            let (p0, p1, center) = (
                reader.screen_to_photo(0.0, 0.0),
                reader.screen_to_photo(1280.0, 0.0),
                reader.screen_to_photo(660.0, 205.0),
            );
            let (Outcome::Ok, Some(p0), Some(p1), Some(center)) =
                (e2e::judge(a, &clean).0, p0, p1, center)
            else {
                skipped.lock().unwrap().push(a.name.clone());
                continue;
            };
            let ppx = ((p1[0] - p0[0]).hypot(p1[1] - p0[1]) / 1280.0) as f64;
            let mut mine = Vec::new();
            for &(kind, level) in &levels {
                let out = degrade(&img, kind, level, ppx, center);
                let r = reader.read(&out.px, out.w as u32, out.h as u32).unwrap();
                let (outcome, index_ok, _) = e2e::judge(a, &r);
                // 何個目かは、自信なしなら画面で「読み取りが曖昧」と出て確かめてもらえるので、自信ありで違うときだけ数える
                let index_sure =
                    matches!(&r, ReadResult::Read { fields, .. } if fields.index.confident);
                let index_slip = index_ok == Some(false) && index_sure;
                if let Outcome::Wrong(m) = &outcome {
                    println!("すり抜け: {} {kind} {level}: {m}", a.name);
                }
                if index_slip {
                    println!("何個目かのすり抜け: {} {kind} {level}", a.name);
                }
                if let (Ok(dir), true) = (
                    std::env::var("RENKIN_DUMP"),
                    index_slip || matches!(outcome, Outcome::Wrong(_)),
                ) {
                    dump(
                        Path::new(&dir),
                        &a.name,
                        kind,
                        level,
                        &out,
                        reader.aligned_image(),
                    );
                }
                mine.push((a.name.clone(), kind, level, outcome, index_slip));
            }
            println!(
                "{:60} {:5.1}s  (全体 {:4.0}s, {}/{})",
                a.name,
                t.elapsed().as_secs_f32(),
                started.elapsed().as_secs_f32(),
                i + 1,
                all.len()
            );
            rows.lock().unwrap().extend(mine);
        }
    };
    std::thread::scope(|s| {
        let threads = std::env::var("RENKIN_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);
        for _ in 0..threads {
            s.spawn(work);
        }
    });

    let rows = rows.into_inner().unwrap();
    let skipped = skipped.into_inner().unwrap();
    let n = all.len() - skipped.len();
    println!(
        "\n劣化前に期待どおり読めなかったので使わなかった画像 {} 枚: {:?}",
        skipped.len(),
        skipped
    );
    let mut slips = 0;
    for &(kind, ls) in LEVELS {
        if !levels.iter().any(|l| l.0 == kind) {
            continue;
        }
        println!("■ {kind}");
        for &l in ls {
            let rs: Vec<&Row> = rows.iter().filter(|r| r.1 == kind && r.2 == l).collect();
            let ok = rs.iter().filter(|r| r.3 == Outcome::Ok).count();
            let retake = rs
                .iter()
                .filter(|r| matches!(r.3, Outcome::Retake(_)))
                .count();
            let wrong = rs
                .iter()
                .filter(|r| matches!(r.3, Outcome::Wrong(_)))
                .count();
            let index_slips = rs.iter().filter(|r| r.4).count();
            slips += wrong + index_slips;
            println!(
                "  {l:>5}: OK {ok:>3}/{n}  撮り直し {retake:>3}  すり抜け {wrong:>2}  何個目かのすり抜け {index_slips:>2}"
            );
        }
    }
    println!("\n全体 {:.0} 秒", started.elapsed().as_secs_f32());
    assert_eq!(slips, 0, "すり抜けがある (上の一覧)");
}
