//! 拡大縮小と平行移動
//!
//! OpenCV (Apache License 2.0) の modules/imgproc/src/resize.cpp の線形補間・面積の平均 (INTER_AREA)・3 次補間 (重みの求め方と、resizeGeneric_ / resizeArea_ / VResizeLinearVec_32s8u / VResizeCubicVec_32s8u の丸め) に合わせて、Rust で書いたもの。
//! ライセンスと著作権表示はリポジトリの THIRD_PARTY_LICENSES/OpenCV を参照

use super::image::{Image, Rgb, RgbSource};

const COEF_BITS: i32 = 11;
const COEF_SCALE: f32 = (1 << COEF_BITS) as f32;

/// 出力の 1 つの座標について、元の画像のどの 2 画素をどの重みで混ぜるか (OpenCV の線形補間と同じ求め方)
fn linear_coefs(dst_len: usize, src_len: usize, inv_scale: f64) -> Vec<(usize, i32, i32)> {
    let scale = 1.0 / inv_scale;
    (0..dst_len)
        .map(|d| {
            let f = ((d as f64 + 0.5) * scale - 0.5) as f32;
            let mut s = f.floor() as i64;
            let mut f = f - s as f32;
            if s < 0 {
                s = 0;
                f = 0.0;
            }
            if s >= src_len as i64 - 1 {
                s = src_len as i64 - 1;
                f = 0.0;
            }
            let a0 = ((1.0 - f) * COEF_SCALE).round_ties_even() as i32;
            let a1 = (f * COEF_SCALE).round_ties_even() as i32;
            (s as usize, a0, a1)
        })
        .collect()
}

/// 線形補間で scale 倍にする (cv2.resize(src, None, fx=scale, fy=scale, interpolation=INTER_LINEAR))。
/// OpenCV 自身の 8 ビット版 (imgproc/src/resize.cpp の resizeGeneric_ と SIMD 版の丸め) と同じく、重みを 2^11 倍した
/// 整数で計算する。Python 版が使っている OpenCV (5.0、x86) は Intel IPP 入りで、拡大は IPP が計算するので、
/// ごく一部の画素が ±1 違う (21x100 を 1.03 倍で 2100 画素中 27 画素)。縮小は OpenCV 自身が計算し、同じ値になる
pub fn resize_linear(src: &Image<u8>, scale: f64) -> Image<u8> {
    let (sw, sh) = (src.width(), src.height());
    let dw = (sw as f64 * scale).round_ties_even() as usize;
    let dh = (sh as f64 * scale).round_ties_even() as usize;
    let xs = linear_coefs(dw, sw, scale);
    let ys = linear_coefs(dh, sh, scale);
    // 横方向: 元の各行を、出力の幅に混ぜた整数 (2^11 倍)
    let hrow = |y: usize| -> Vec<i32> {
        let row = src.row(y);
        xs.iter()
            .map(|&(s, a0, a1)| {
                let next = if s + 1 < sw { row[s + 1] as i32 } else { 0 };
                row[s] as i32 * a0 + next * a1
            })
            .collect()
    };
    let mut out = Image::new(dw, dh, 0u8);
    for (dy, &(sy, b0, b1)) in ys.iter().enumerate() {
        let r0 = hrow(sy);
        let r1 = if sy + 1 < sh {
            hrow(sy + 1)
        } else {
            vec![0; dw]
        };
        for dx in 0..dw {
            // OpenCV の SIMD 版と同じ丸め方 (下位 4 ビットを先に捨ててから掛ける)
            let v = (((b0 * (r0[dx] >> 4)) >> 16) + ((b1 * (r1[dx] >> 4)) >> 16) + 2) >> 2;
            out.set(dx, dy, v.clamp(0, 255) as u8);
        }
    }
    out
}

/// (dx, dy) だけずれた画像を戻す: 出力の (x, y) に元の (x + dx, y + dy) を入れる。画像の外は 0
/// (cv2.warpAffine(src, [[1, 0, -dx], [0, 1, -dy]]) と同じ)
pub fn translate(src: &Image<u8>, dx: i32, dy: i32) -> Image<u8> {
    let (w, h) = (src.width() as i64, src.height() as i64);
    let mut out = Image::new(src.width(), src.height(), 0u8);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = (x + dx as i64, y + dy as i64);
            if (0..w).contains(&sx) && (0..h).contains(&sy) {
                out.set(x as usize, y as usize, src.get(sx as usize, sy as usize));
            }
        }
    }
    out
}

/// 縮小 (INTER_AREA) の 1 方向の表: (出力の位置, 元の位置, 重み)。OpenCV の computeResizeAreaTab と同じ
fn area_tab(src_len: usize, dst_len: usize, scale: f64) -> Vec<(usize, usize, f32)> {
    let mut tab = Vec::new();
    for d in 0..dst_len {
        let fs1 = d as f64 * scale;
        let fs2 = fs1 + scale;
        let cell = scale.min(src_len as f64 - fs1);
        let mut s2 = fs2.floor() as i64;
        s2 = s2.min(src_len as i64 - 1);
        let s1 = (fs1.ceil() as i64).min(s2);
        if s1 as f64 - fs1 > 1e-3 {
            tab.push((d, (s1 - 1) as usize, ((s1 as f64 - fs1) / cell) as f32));
        }
        for s in s1..s2 {
            tab.push((d, s as usize, (1.0 / cell) as f32));
        }
        if fs2 - s2 as f64 > 1e-3 {
            tab.push((
                d,
                s2 as usize,
                ((fs2 - s2 as f64).min(1.0).min(cell) / cell) as f32,
            ));
        }
    }
    tab
}

/// 1 行を横に足し合わせる。表は出力の位置の順に並んでいるので、出力 1 画素ぶんを手元で足してから書く
/// (0 から表の順に足すのは同じなので、値は buf に直接足していくのと同じ)
#[inline(never)]
fn area_row(row: &[Rgb], xtab: &[(usize, usize, f32)], buf: &mut [[f32; 3]]) {
    buf.iter_mut().for_each(|b| *b = [0.0; 3]);
    let Some(&(mut d, _, _)) = xtab.first() else {
        return;
    };
    let mut acc = [0f32; 3];
    for &(dx, sx, alpha) in xtab {
        if dx != d {
            buf[d] = acc;
            acc = [0.0; 3];
            d = dx;
        }
        let p = row[sx];
        for c in 0..3 {
            acc[c] += p[c] as f32 * alpha;
        }
    }
    buf[d] = acc;
}

/// 面積の平均で縮小する (cv2.resize(src, None, fx=scale, fy=scale, interpolation=INTER_AREA)、倍率が整数分の 1 でないとき)。
/// OpenCV の resizeArea_ と同じく、float32 で横、縦の順に足し合わせる
pub fn resize_area(src: &impl RgbSource, scale: f64) -> Image<Rgb> {
    let (sw, sh) = (src.width(), src.height());
    let dw = (sw as f64 * scale).round_ties_even() as usize;
    let dh = (sh as f64 * scale).round_ties_even() as usize;
    let inv = 1.0 / scale;
    let xtab = area_tab(sw, dw, inv);
    let ytab = area_tab(sh, dh, inv);
    let mut out = Image::new(dw, dh, [0u8; 3]);
    let mut sum = vec![[0f32; 3]; dw];
    let mut buf = vec![[0f32; 3]; dw];
    let mut prev_dy = ytab.first().map_or(0, |t| t.0);
    let store = |out: &mut Image<Rgb>, dy: usize, sum: &[[f32; 3]]| {
        for (dx, v) in sum.iter().enumerate() {
            out.set(
                dx,
                dy,
                v.map(|c| c.round_ties_even().clamp(0.0, 255.0) as u8),
            );
        }
    };
    let mut row = vec![[0u8; 3]; sw];
    let mut buf_sy = usize::MAX;
    for &(dy, sy, beta) in &ytab {
        // 横に足した結果は元の行だけで決まるので、境目で同じ行が続けて来たら使い回す
        if sy != buf_sy {
            src.read_row(sy, &mut row);
            area_row(&row, &xtab, &mut buf);
            buf_sy = sy;
        }
        if dy != prev_dy {
            store(&mut out, prev_dy, &sum);
            for dx in 0..dw {
                sum[dx] = buf[dx].map(|b| beta * b);
            }
            prev_dy = dy;
        } else {
            for dx in 0..dw {
                for c in 0..3 {
                    sum[dx][c] += beta * buf[dx][c];
                }
            }
        }
    }
    store(&mut out, prev_dy, &sum);
    out
}

/// 3 次補間の重み (OpenCV の interpolateCubic、A = -0.75)
fn cubic_coefs(x: f32) -> [f32; 4] {
    const A: f32 = -0.75;
    let c0 = ((A * (x + 1.0) - 5.0 * A) * (x + 1.0) + 8.0 * A) * (x + 1.0) - 4.0 * A;
    let c1 = ((A + 2.0) * x - (A + 3.0)) * x * x + 1.0;
    let c2 = ((A + 2.0) * (1.0 - x) - (A + 3.0)) * (1.0 - x) * (1.0 - x) + 1.0;
    [c0, c1, c2, 1.0 - c0 - c1 - c2]
}

/// 1 方向の表: (元の位置の 4 つ (端は端の値), 重み 4 つを 2^11 倍した整数)
fn cubic_tab(src_len: usize, dst_len: usize, inv_scale: f64) -> Vec<([usize; 4], [i32; 4])> {
    let scale = 1.0 / inv_scale;
    (0..dst_len)
        .map(|d| {
            let f = ((d as f64 + 0.5) * scale - 0.5) as f32;
            let s = f.floor() as i64;
            let coefs = cubic_coefs(f - s as f32);
            let idx = [0, 1, 2, 3].map(|k| (s + k - 1).clamp(0, src_len as i64 - 1) as usize);
            (
                idx,
                coefs.map(|c| (c * COEF_SCALE).round_ties_even() as i32),
            )
        })
        .collect()
}

/// 3 次補間で拡大する (cv2.resize(src, None, fx=scale, fy=scale, interpolation=INTER_CUBIC))。
/// OpenCV の 8 ビット版と同じく、横は重みを 2^11 倍した整数で、縦は float32 (VResizeCubicVec_32s8u) で計算する。
/// simd_lanes は OpenCV が縦を SIMD で計算する幅 (int16 の数)。それに満たない行の端は、整数で計算する。
/// Python 版の OpenCV (Intel IPP 入り) とは、拡大なので ±1 違う値がある (21x100 程度の画像で 6% ほど)
pub fn resize_cubic(src: &impl RgbSource, scale: f64, simd_lanes: usize) -> Image<Rgb> {
    let (sw, sh) = (src.width(), src.height());
    let dw = (sw as f64 * scale).round_ties_even() as usize;
    let dh = (sh as f64 * scale).round_ties_even() as usize;
    let xs = cubic_tab(sw, dw, scale);
    let ys = cubic_tab(sh, dh, scale);
    let mut line = vec![[0u8; 3]; sw];
    let hrow = |y: usize| -> Vec<i32> {
        src.read_row(y, &mut line);
        let mut row = Vec::with_capacity(dw * 3);
        for (idx, a) in &xs {
            let p = idx.map(|i| line[i]);
            row.extend((0..3).map(|c| (0..4).map(|k| p[k][c] as i32 * a[k]).sum::<i32>()));
        }
        row
    };
    let rows: Vec<Vec<i32>> = (0..sh).map(hrow).collect();
    let width = dw * 3;
    let simd_end = width / simd_lanes * simd_lanes;
    let fscale = 1.0f32 / (COEF_SCALE * COEF_SCALE);
    let mut out = Image::new(dw, dh, [0u8; 3]);
    for (dy, (idx, b)) in ys.iter().enumerate() {
        let s = idx.map(|i| &rows[i]);
        let bf = b.map(|v| v as f32 * fscale);
        let orow = &mut out.data_mut()[dy * dw..(dy + 1) * dw];
        for i in 0..width {
            let v = if i < simd_end {
                let p = [0, 1, 2, 3].map(|k| s[k][i] as f32);
                p[0].mul_add(
                    bf[0],
                    p[1].mul_add(bf[1], p[2].mul_add(bf[2], p[3] * bf[3])),
                )
                .round_ties_even() as i32
            } else {
                (b[0] * s[0][i] + b[1] * s[1][i] + b[2] * s[2][i] + b[3] * s[3][i] + (1 << 21))
                    >> 22
            };
            orow[i / 3][i % 3] = v.clamp(0, 255) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{json_fixture, json_image, json_u8};

    #[test]
    fn resize_matches_opencv() {
        let f = json_fixture("cv/resize.json");
        let src = json_image(&f["src"], json_u8);
        for s in ["0.97", "1.03", "0.5", "2.0"] {
            let want = json_image(&f[s], json_u8);
            let got = resize_linear(&src, s.parse().unwrap());
            assert_eq!(
                (got.width(), got.height()),
                (want.width(), want.height()),
                "{s} の大きさ"
            );
            let diff: Vec<i32> = got
                .data()
                .iter()
                .zip(want.data())
                .map(|(&a, &b)| a as i32 - b as i32)
                .filter(|&d| d != 0)
                .collect();
            if s.parse::<f64>().unwrap() <= 1.0 {
                assert!(diff.is_empty(), "{s}: {} 画素違う", diff.len());
            } else {
                // 拡大は Python 版の OpenCV では Intel IPP が計算し、丸めが違う。±1 までで、ごく一部だけ
                assert!(
                    diff.iter().all(|d| d.abs() <= 1),
                    "{s}: 2 以上違う画素がある"
                );
                assert!(
                    diff.len() * 20 <= got.data().len(),
                    "{s}: {} 画素違う (5% を超える)",
                    diff.len()
                );
            }
        }
    }

    fn rgb_diff(got: &Image<Rgb>, want: &Image<Rgb>) -> (usize, i32) {
        assert_eq!((got.width(), got.height()), (want.width(), want.height()));
        let d: Vec<i32> = got
            .data()
            .iter()
            .zip(want.data())
            .flat_map(|(a, b)| (0..3).map(move |c| a[c] as i32 - b[c] as i32))
            .filter(|&d| d != 0)
            .collect();
        (d.len(), d.iter().map(|v| v.abs()).max().unwrap_or(0))
    }

    #[test]
    fn area_and_cubic_match_opencv() {
        let f = crate::testutil::json_fixture("cv/align_ops.json");
        let src = crate::testutil::json_rgb(&f["src"]);
        for s in ["0.496", "0.3125"] {
            let (n, worst) = rgb_diff(
                &resize_area(&src, s.parse().unwrap()),
                &crate::testutil::json_rgb(&f[format!("area{s}")]),
            );
            println!("INTER_AREA {s}: 違う値 {n} 個、最大 {worst}");
            assert!(n == 0, "INTER_AREA {s}");
        }
        // 拡大は Python 版の OpenCV では Intel IPP が計算するので、±1 違う値がある (resize_linear と同じ)
        let (n, worst) = rgb_diff(
            &resize_cubic(&src, 1.5625, 16),
            &crate::testutil::json_rgb(&f["cubic1.5625"]),
        );
        println!("INTER_CUBIC: 違う値 {n} 個、最大 {worst}");
        assert!(
            worst <= 1 && n * 10 <= src.width() * src.height() * 3 * 2,
            "INTER_CUBIC"
        );
    }

    #[test]
    fn translate_matches_opencv() {
        let f = json_fixture("cv/translate.json");
        let src = json_image(&f["src"], json_u8);
        for (dx, dy) in [(3, -2), (-4, 1), (0, 0)] {
            assert_eq!(
                translate(&src, dx, dy),
                json_image(&f[format!("{dx},{dy}")], json_u8),
                "{dx},{dy}"
            );
        }
    }
}
