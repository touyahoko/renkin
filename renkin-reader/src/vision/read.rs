//! 鑑定画面 (位置合わせ後の 1280x720) の欄の読み取り。experimental-charm-reader の reader.py を移したもの

use super::assets;
use super::cv::color;
use super::cv::components::connected_components;
use super::cv::filter;
use super::cv::image::{Image, RgbaView};
use super::cv::matching::{correlation, match_template, max_loc};
use super::cv::resize::{resize_linear, translate};
use crate::types::SkillId;

/// 見本は 1280x720 のキャプチャから切り出しているので等倍。写真の補正誤差のぶん少しだけ振る
pub const TMPL_SCALES: [f64; 3] = [0.97, 1.0, 1.03];
/// 倍率 1 の見本の添字
pub const SCALE_ONE: usize = 1;
/// 文字より大きいゆるやかな明るさの変化を引く (トップハット) 窓の大きさ。小さいと画数の多い漢字の詰まった部分まで削れる
pub const TOPHAT: usize = 25;
/// パネル左の固定ラベル「固有スキル1」「固有スキル2」「スロット」のかたまり (左, 上, 右, 下)
pub const LABEL_BLOCK: (usize, usize, usize, usize) = (476, 166, 570, 247);
/// 埋め込んだラベルの見本 (assets/label.png) の余白。トップハット (収縮してから膨張) が届く 24px
pub const LABEL_MARGIN: usize = 24;

/// 欄の位置 (左, 上, 右, 下)。行の区切り線 (y=163, 192, 219) の内側だけを切り出す
pub const NAME1: Rect = (655, 165, 800, 191);
pub const PTS1: Rect = (795, 165, 850, 191);
pub const NAME2: Rect = (655, 194, 800, 218);
pub const PTS2: Rect = (795, 194, 850, 218);
/// スキル名はパネルの中で左揃え。名前の左端の位置 (画面の x) と、そこから横に何 px まで見本を探すか。
/// 横を欄全体で探すと、反射で名前の先頭が消えたとき、残った文字と同じ字の短い名前 (「氷属性攻撃」→「攻撃」) に
/// 自信ありで当たってしまう
pub const NAME_LEFT: usize = 667;
pub const NAME_SEARCH: usize = 4;
/// パネルのラベルを、予想位置から ±何 px 探すか
pub const LABEL_SEARCH: usize = 30;
/// ラベルの一致度がこれ未満なら、パネルの位置を確かめられない (ずれを戻さない)
pub const LABEL_MIN_SCORE: f64 = 0.5;

/// (左, 上, 右, 下)。右と下は含まない
pub type Rect = (usize, usize, usize, usize);

/// 明るい細い線 (文字) だけを残し、にじみや明るさのむらを取り除く
pub fn tophat(g: &Image<u8>) -> Image<u8> {
    filter::tophat(g, TOPHAT)
}

/// g 全体にトップハットをかけたうちの rect の部分。rect のまわり 24px (トップハットが届く範囲) だけを切り出して
/// 計算するので、全体にかけるより速く、値は全体にかけたときと同じ
pub fn tophat_region(g: &Image<u8>, rect: Rect) -> Image<u8> {
    let (x0, y0, x1, y1) = rect;
    let m = TOPHAT - 1; // 収縮と膨張で半径 12 ずつ
    let (ex0, ey0) = (x0.saturating_sub(m), y0.saturating_sub(m));
    let (ex1, ey1) = ((x1 + m).min(g.width()), (y1 + m).min(g.height()));
    tophat(&g.crop(ex0, ey0, ex1 - ex0, ey1 - ey0)).crop(x0 - ex0, y0 - ey0, x1 - x0, y1 - y0)
}

/// 中間 (最小と最大の平均) より明るい画素を文字とし、横幅の 6 割を超えて光る行 (区切り線) は除いたもの
pub fn ink_mask(g: &Image<u8>) -> Image<bool> {
    let (lo, hi) = g
        .data()
        .iter()
        .fold((u8::MAX, 0u8), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let thr = (lo as u16 + hi as u16) / 2;
    let mut m = g.map(|v| v as u16 > thr);
    for y in 0..m.height() {
        if m.row(y).iter().filter(|&&b| b).count() * 10 > m.width() * 6 {
            for x in 0..m.width() {
                m.set(x, y, false);
            }
        }
    }
    m
}

/// 文字の部分 (ink_mask) の外接長方形を切り出す
pub fn glyph(g: &Image<u8>) -> Image<u8> {
    let m = ink_mask(g);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..m.height() {
        for x in 0..m.width() {
            if m.get(x, y) {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    assert!(x0 <= x1, "文字の画素が無い");
    g.crop(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
}

/// 3 つの倍率 (TMPL_SCALES) の見本
pub type Scaled = [Image<u8>; 3];

fn scaled(g: Image<u8>) -> Scaled {
    let small = resize_linear(&g, TMPL_SCALES[0]);
    let large = resize_linear(&g, TMPL_SCALES[2]);
    [small, g, large]
}

/// 下ごしらえ済みの見本 (experimental-charm-reader の templates()、sign_templates() など)
pub struct Templates {
    /// (スキル番号, 3 つの倍率の見本)。番号の小さい順
    pub skills: Vec<(SkillId, Scaled)>,
    /// (ポイントの絶対値, 3 つの倍率の見本)。符号を除いた数字だけ
    pub digits: Vec<(u8, Scaled)>,
    /// 符号「+」「-」 (トップハット済み)
    pub plus: Image<f32>,
    pub minus: Image<f32>,
    /// スロットの丸と横線 (そのまま)
    pub circle: Image<f32>,
    pub dash: Image<f32>,
    /// パネルの固定ラベル (トップハット済み、LABEL_BLOCK の大きさ)
    pub label: Image<u8>,
}

impl Templates {
    pub fn load() -> Self {
        let prepare = |a: &assets::Asset| scaled(glyph(&tophat(&a.image())));
        let key = |a: &assets::Asset| {
            a.name
                .parse::<u16>()
                .unwrap_or_else(|_| panic!("見本の名前が番号でない: {}", a.name))
        };
        let named = |list: &[assets::Asset], name: &str| {
            list.iter().find(|a| a.name == name).unwrap().image()
        };
        let (x0, y0, x1, y1) = LABEL_BLOCK;
        Self {
            skills: assets::SKILLS
                .iter()
                .map(|a| (key(a), prepare(a)))
                .collect(),
            digits: assets::DIGITS
                .iter()
                .map(|a| (key(a) as u8, prepare(a)))
                .collect(),
            plus: tophat(&named(assets::SIGNS, "plus")).map(f32::from),
            minus: tophat(&named(assets::SIGNS, "minus")).map(f32::from),
            circle: named(assets::SLOTS, "circle").map(f32::from),
            dash: named(assets::SLOTS, "dash").map(f32::from),
            label: tophat(&assets::LABEL.image()).crop(
                LABEL_MARGIN,
                LABEL_MARGIN,
                x1 - x0,
                y1 - y0,
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// 照合と比べ直し
// ---------------------------------------------------------------------------

/// 当たった範囲の外に残った文字の割合 x これ を、照合の一致度から引く
pub const LEFTOVER_PENALTY: f64 = 1.0;
/// 1 位と 2 位の差がこれ未満なら、2 つの見本で画素が違うところ (とその周り) だけで比べ直す
pub const RECHECK_MARGIN: f64 = 0.2;
/// 違う画素の周りに広げる幅 (正方形の一辺)
pub const RECHECK_GROW: usize = 5;
/// スキル名の 1 位と 2 位の見本で、違う画素がどちらかに文字がある画素の何割か。これを超える組み合わせは似ていない
/// (全部の字が違う) ので、比べ直さず名前全体の差だけで判定する。数字にはかけない
pub const RECHECK_MAX_DIFF: f64 = 0.3;
/// 候補として返す数
pub const CANDIDATES: usize = 3;

/// 列ごとの文字の画素数 (ink_mask)
fn ink_columns(roi: &Image<u8>) -> Vec<u32> {
    let m = ink_mask(roi);
    (0..m.width())
        .map(|x| (0..m.height()).filter(|&y| m.get(x, y)).count() as u32)
        .collect()
}

/// roi の上で見本 t をずらし、(一番高い一致度, その位置 (x, y)) を返す。x_range = (最小, 最大) があれば、
/// 見本の左端がその範囲 (両端を含む) の位置だけで探す。見本が roi より大きいか、探せる位置が無ければ None
fn match_best(
    roi: &Image<u8>,
    t: &Image<u8>,
    x_range: Option<(usize, usize)>,
) -> Option<(f64, (usize, usize))> {
    if t.width() > roi.width() || t.height() > roi.height() {
        return None;
    }
    let last = roi.width() - t.width();
    let (lo, hi) = x_range.map_or((0, last), |(lo, hi)| (lo, hi.min(last)));
    if lo > hi {
        return None;
    }
    let res = match_template(roi, t, Some(lo..hi + 1));
    let (v, (x, y)) = max_loc(&res);
    Some((v as f64, (x + lo, y)))
}

/// 見本 t (の値) を、shape の大きさの画像の loc (左上、負もありうる) に置く。外側は見本の最小値。はみ出た分は切る
fn place(shape: (usize, usize), t: &Image<u8>, loc: (i64, i64)) -> Image<f32> {
    let fill = *t.data().iter().min().unwrap() as f32;
    let (w, h) = shape;
    let mut c = Image::new(w, h, fill);
    for ty in 0..t.height() {
        for tx in 0..t.width() {
            let (x, y) = (loc.0 + tx as i64, loc.1 + ty as i64);
            if (0..w as i64).contains(&x) && (0..h as i64).contains(&y) {
                c.set(x as usize, y as usize, t.get(tx, ty) as f32);
            }
        }
    }
    c
}

/// 最小と最大の平均 (小数) より明るい画素
fn ink<T: Copy + Into<f64>>(t: &Image<T>) -> Image<bool> {
    let (lo, hi) = t.data().iter().fold((f64::MAX, f64::MIN), |(lo, hi), &v| {
        (lo.min(v.into()), hi.max(v.into()))
    });
    let thr = (lo + hi) / 2.0;
    t.map(|v| v.into() > thr)
}

/// 見本 tb を ta に重ねたとき、文字の画素が一番重なるずれ (dx, dy)。-3〜+3 px を、dy、dx の順に小さいほうから試す
fn pair_offset(ta: &Image<u8>, tb: &Image<u8>) -> (i64, i64) {
    const R: i64 = 3;
    let (ia, ib) = (ink(ta), ink(tb));
    let mut best = (-1i64, 0, 0);
    for dy in -R..=R {
        for dx in -R..=R {
            let mut v = 0i64;
            for ya in 0..ia.height() as i64 {
                for xa in 0..ia.width() as i64 {
                    let (yb, xb) = (ya - dy, xa - dx);
                    if ia.get(xa as usize, ya as usize)
                        && (0..ib.height() as i64).contains(&yb)
                        && (0..ib.width() as i64).contains(&xb)
                        && ib.get(xb as usize, yb as usize)
                    {
                        v += 1;
                    }
                }
            }
            if v > best.0 {
                best = (v, dx, dy);
            }
        }
    }
    (best.1, best.2)
}

/// 倍率 1 の見本 a と b を文字が一番重なる位置に重ねたとき、違う画素が、どちらかに文字がある画素の何割か
fn pair_diff(ta: &Image<u8>, tb: &Image<u8>) -> f64 {
    let (dx, dy) = pair_offset(ta, tb);
    let w = (ta.width() as i64).max(tb.width() as i64 + dx) as usize + 8;
    let h = (ta.height() as i64).max(tb.height() as i64 + dy) as usize + 8;
    let ia = ink(&place((w, h), ta, (4, 4)));
    let ib = ink(&place((w, h), tb, (4 + dx, 4 + dy)));
    let (mut xor, mut or) = (0usize, 0usize);
    for (&a, &b) in ia.data().iter().zip(ib.data()) {
        xor += (a ^ b) as usize;
        or += (a | b) as usize;
    }
    xor as f64 / or.max(1) as f64
}

/// 1 位 (a) と 2 位 (b) の倍率 1 の見本で、文字の画素が違うところ (周りを少し広げる) だけを roi から取り出し、
/// それぞれとの相関係数を返す。違うところが無ければ None。
/// a は roi で一番合う位置に置き、b は a と文字が一番重なる位置 (見本どうしで求める) に置く
fn recheck(
    roi: &Image<u8>,
    ta: &Image<u8>,
    tb: &Image<u8>,
    x_range: Option<(usize, usize)>,
) -> Option<(f64, f64)> {
    let (_, (lx, ly)) = match_best(roi, ta, x_range)?;
    let (dx, dy) = pair_offset(ta, tb);
    let shape = (roi.width(), roi.height());
    let ca = place(shape, ta, (lx as i64, ly as i64));
    let cb = place(shape, tb, (lx as i64 + dx, ly as i64 + dy));
    let (ia, ib) = (ink(&ca), ink(&cb));
    let diff = Image::from_vec(
        shape.0,
        shape.1,
        ia.data()
            .iter()
            .zip(ib.data())
            .map(|(&a, &b)| a ^ b)
            .collect(),
    );
    let mask = filter::dilate(&diff, RECHECK_GROW);
    let idx: Vec<usize> = (0..mask.data().len()).filter(|&i| mask.data()[i]).collect();
    if idx.len() < 4 {
        return None;
    }
    let pick = |img: &[f64]| idx.iter().map(|&i| img[i]).collect::<Vec<f64>>();
    let r = pick(&roi.data().iter().map(|&v| v as f64).collect::<Vec<_>>());
    let a = pick(&ca.data().iter().map(|&v| v as f64).collect::<Vec<_>>());
    let b = pick(&cb.data().iter().map(|&v| v as f64).collect::<Vec<_>>());
    Some((correlation(&r, &a), correlation(&r, &b)))
}

/// 照合の結果
#[derive(Debug, Clone, PartialEq)]
pub struct Scored<T> {
    pub value: T,
    /// 1 位の点数
    pub score: f64,
    /// 1 位と 2 位の差 (比べ直しの結果を反映したもの)
    pub margin: f64,
    /// 点数の高い順の上位 (1 位を含む)
    pub candidates: Vec<(T, f64)>,
}

/// 各候補の一致度から「当たった範囲の外に残った文字の割合」を引いた点数で選ぶ。
/// 1 位と 2 位が近ければ、2 つの見本で画素が違うところだけで比べ直す (max_diff があれば、似ている組み合わせだけ)。
/// 比べ直しでも同じ 1 位なら差は名前全体と比べ直しの大きいほう、1 位が入れ替わったら差を 0 にする
pub fn best_match<K: Copy>(
    roi: &Image<u8>,
    tmpls: &[(K, Scaled)],
    x_range: Option<(usize, usize)>,
    max_diff: Option<f64>,
) -> Scored<K> {
    let cols = ink_columns(roi);
    let total = cols.iter().sum::<u32>().max(1) as f64;
    let mut ranked: Vec<(usize, f64)> = tmpls
        .iter()
        .enumerate()
        .map(|(n, (_, scaled))| {
            let mut best = -1.0;
            for t in scaled {
                let Some((v, (x, _))) = match_best(roi, t, x_range) else {
                    continue;
                };
                let inside: u32 = cols[x.saturating_sub(2)..(x + t.width() + 2).min(cols.len())]
                    .iter()
                    .sum();
                let score = v - LEFTOVER_PENALTY * (total - inside as f64) / total;
                if score > best {
                    best = score;
                }
            }
            (n, best)
        })
        .collect();
    // 同点なら見本の順 (Python 版の sorted と同じく安定な並べ替え)
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let (a, sa) = ranked[0];
    let candidates = ranked
        .iter()
        .take(CANDIDATES)
        .map(|&(n, s)| (tmpls[n].0, s))
        .collect();
    let mut margin = ranked.get(1).map_or(sa + 1.0, |&(_, sb)| sa - sb);
    if let Some(&(b, sb)) = ranked.get(1) {
        let (ta, tb) = (&tmpls[a].1[SCALE_ONE], &tmpls[b].1[SCALE_ONE]);
        if sa - sb < RECHECK_MARGIN
            && max_diff.is_none_or(|m| pair_diff(ta, tb) <= m)
            && let Some((la, lb)) = recheck(roi, ta, tb, x_range)
        {
            margin = if la >= lb {
                (sa - sb).max(la - lb)
            } else {
                0.0
            };
        }
    }
    Scored {
        value: tmpls[a].0,
        score: sa,
        margin,
        candidates,
    }
}

// ---------------------------------------------------------------------------
// 第 2 スキルなし・ポイント・スロット・何個目か
// ---------------------------------------------------------------------------

/// 文字の画素のうち、横線の行 (の上下 2 行まで) に入っている割合がこれ以上なら横線
pub const DASH_INK_IN_BAND: f64 = 0.85;

/// 第 2 スキル欄が「-----」か。切り出し幅の 25% 以上が光る行が 6 行以内に収まり、かつ文字の画素のほとんどが
/// その行の範囲に入っていれば横線とみなす (後の条件が無いと、短い名前の強い横画 1 本を横線と取り違える)
pub fn is_dashes(roi: &Image<u8>) -> bool {
    let m = ink_mask(roi);
    let rows: Vec<usize> = (0..m.height())
        .map(|y| m.row(y).iter().filter(|&&b| b).count())
        .collect();
    let strong: Vec<usize> = (0..rows.len())
        .filter(|&y| rows[y] as f64 >= 0.25 * m.width() as f64)
        .collect();
    let (Some(&first), Some(&last)) = (strong.first(), strong.last()) else {
        return false;
    };
    if last - first + 1 > 6 {
        return false;
    }
    let band: usize = rows[first.saturating_sub(2)..(last + 3).min(rows.len())]
        .iter()
        .sum();
    band as f64 >= DASH_INK_IN_BAND * rows.iter().sum::<usize>().max(1) as f64
}

/// これより高い字のかたまりは数字 (符号は 14px まで)
pub const SIGN_MAX_HEIGHT: usize = 16;
/// 符号の見本を、かたまりの中心から何 px ずらして探すか
pub const SIGN_SEARCH: usize = 2;
/// 「+」と「-」の一致度の差がこれ未満なら自信なし
pub const SIGN_MIN_DIFF: f64 = 0.2;
/// よく合うほうの一致度がこれ未満なら自信なし
pub const SIGN_MIN_SCORE: f64 = 0.6;
/// 符号のかたまりの幅がこれを超えたら自信なし (ごみや数字とくっついた形)
pub const SIGN_MAX_WIDTH: usize = 15;
/// 画素数がこれ未満か、幅が SPECK_WIDTH 未満のかたまりは、ごみとして無視する
pub const SPECK_MIN: usize = 4;
pub const SPECK_WIDTH: usize = 3;

/// 字のかたまりの (左, 上, 右, 下)。右と下は含まない
type Boxed = (usize, usize, usize, usize);

/// 二値画像を字のかたまりに分け、左から順に返す。一番背の高いかたまり (数字のはず) の上下の範囲から中心が外れた
/// かたまりは捨て、横の範囲が狭いほうの幅の半分以上重なるかたまりは同じ字のかけらとしてまとめる
fn glyphs(m: &Image<bool>) -> Vec<Boxed> {
    let (_, comps) = connected_components(m);
    let comps: Vec<Boxed> = comps
        .iter()
        .filter(|c| c.area >= SPECK_MIN && c.width >= SPECK_WIDTH || c.height > SIGN_MAX_HEIGHT)
        .map(|c| (c.left, c.top, c.left + c.width, c.top + c.height))
        .collect();
    // 一番背の高いもの。同じ高さなら番号の若いほう (Python 版の max と同じ)
    let Some(tall) = comps
        .iter()
        .copied()
        .reduce(|a, b| if b.3 - b.1 > a.3 - a.1 { b } else { a })
    else {
        return Vec::new();
    };
    let mut kept: Vec<Boxed> = comps
        .into_iter()
        .filter(|c| {
            let center = (c.1 + c.3) as f64 / 2.0;
            tall.1 as f64 <= center && center <= tall.3 as f64
        })
        .collect();
    kept.sort();
    let mut groups: Vec<Boxed> = Vec::new();
    for c in kept {
        if let Some(g) = groups.last_mut() {
            let overlap = g.2.min(c.2) as f64 - g.0.max(c.0) as f64;
            if overlap >= 0.5 * (g.2 - g.0).min(c.2 - c.0) as f64 {
                *g = (g.0.min(c.0), g.1.min(c.1), g.2.max(c.2), g.3.max(c.3));
                continue;
            }
        }
        groups.push(c);
    }
    groups
}

/// ポイントの欄を符号と数字に分け、(符号のかたまり, 数字の左端の x) を返す。数字が無ければ (None, 0)
fn split_sign(roi: &Image<u8>) -> (Option<Boxed>, usize) {
    let gs = glyphs(&ink_mask(roi));
    match gs.iter().position(|g| g.3 - g.1 > SIGN_MAX_HEIGHT) {
        None => (None, 0),
        Some(0) => (None, gs[0].0),
        Some(i) => (Some(gs[i - 1]), gs[i].0),
    }
}

/// 符号のかたまりの中心に見本の中心を合わせて (±SIGN_SEARCH px)、「+」と「-」の見本をよく合うほうの位置に
/// 両方置き、見本の範囲全体で比べて (「+」との一致度, 「-」との一致度) を返す。かたまりが無いか、幅が広すぎれば (0, 0)
fn read_sign(roi: &Image<u8>, sign: Option<Boxed>, t: &Templates) -> (f64, f64) {
    let Some(b) = sign.filter(|b| b.2 - b.0 <= SIGN_MAX_WIDTH) else {
        return (0.0, 0.0);
    };
    let (w, h, r) = (
        t.plus.width() as i64,
        t.plus.height() as i64,
        SIGN_SEARCH as i64,
    );
    let (cx, cy) = (((b.0 + b.2) / 2) as i64, ((b.1 + b.3) / 2) as i64);
    let (x0, y0) = (cx - w / 2 - r, cy - h / 2 - r);
    let mut win = Image::new((w + 2 * r) as usize, (h + 2 * r) as usize, 0f32);
    for y in 0..win.height() as i64 {
        for x in 0..win.width() as i64 {
            let (sx, sy) = (x0 + x, y0 + y);
            if (0..roi.width() as i64).contains(&sx) && (0..roi.height() as i64).contains(&sy) {
                win.set(
                    x as usize,
                    y as usize,
                    roi.get(sx as usize, sy as usize) as f32,
                );
            }
        }
    }
    let (vp, lp) = max_loc(&match_template(&win, &t.plus, None));
    let (vm, lm) = max_loc(&match_template(&win, &t.minus, None));
    let (x, y) = if vm > vp { lm } else { lp };
    let part = win.crop(x, y, w as usize, h as usize);
    let px: Vec<f64> = part.data().iter().map(|&v| v as f64).collect();
    let mean = px.iter().sum::<f64>() / px.len() as f64;
    if px.iter().all(|&v| v == mean) {
        return (0.0, 0.0);
    }
    let corr = |tm: &Image<f32>| {
        correlation(
            &px,
            &tm.data().iter().map(|&v| v as f64).collect::<Vec<_>>(),
        )
    };
    (corr(&t.plus), corr(&t.minus))
}

/// 符号が、はっきり「+」か「-」と言えるか
pub fn sign_ok(plus: f64, minus: f64) -> bool {
    plus.max(minus) >= SIGN_MIN_SCORE && (plus - minus).abs() >= SIGN_MIN_DIFF
}

/// ポイントを読む: (符号付きのポイントと数字の照合の結果, (「+」との一致度, 「-」との一致度))
fn read_points(roi: &Image<u8>, t: &Templates) -> (Scored<i8>, (f64, f64)) {
    let (sign, x) = split_sign(roi);
    let x = x.saturating_sub(2);
    let digits = best_match(
        &roi.crop(x, 0, roi.width() - x, roi.height()),
        &t.digits,
        None,
        None,
    );
    let s = read_sign(roi, sign, t);
    let signed = |d: u8| if s.1 > s.0 { -(d as i8) } else { d as i8 };
    let pts = Scored {
        value: signed(digits.value),
        score: digits.score,
        margin: digits.margin,
        candidates: digits
            .candidates
            .iter()
            .map(|&(d, sc)| (signed(d), sc))
            .collect(),
    };
    (pts, s)
}

/// スロット記号 3 つの左端 x (幅 16) と、上下 (区切り線は y=219, 247)
pub const SLOT_CELLS: [usize; 3] = [580, 604, 628];
pub const SLOT_Y: usize = 224;
/// スロット記号の見本を切り出す範囲 (左, 上, 右, 下)。記号の左端と SLOT_Y からの相対位置
pub const SLOT_CROP: (i64, i64, i64, i64) = (-1, -2, 17, 18);
/// 写真のずれに備えて、見本を何 px ずらして探すか
pub const SLOT_SEARCH: i64 = 4;
/// 丸と横線の一致度の差がこれ未満か、よく合うほうの一致度がこれ未満なら自信なし
pub const SLOT_MIN_DIFF: f64 = 0.2;
pub const SLOT_MIN_SCORE: f64 = 0.5;

/// スロット記号 3 つを丸か横線かに分け、(丸の数, 記号ごとの (丸との一致度, 横線との一致度)) を返す
fn read_slots(mx: &Image<u8>, t: &Templates) -> (u8, [(f64, f64); 3]) {
    let (l, top, r, bottom) = SLOT_CROP;
    let s = SLOT_SEARCH;
    let mut scores = [(0.0, 0.0); 3];
    for (i, &x) in SLOT_CELLS.iter().enumerate() {
        let (ax, ay) = (
            (x as i64 + l - s) as usize,
            (SLOT_Y as i64 + top - s) as usize,
        );
        let area = mx
            .crop(
                ax,
                ay,
                (r - l + 2 * s) as usize,
                (bottom - top + 2 * s) as usize,
            )
            .map(f32::from);
        let best = |tm: &Image<f32>| max_loc(&match_template(&area, tm, None)).0 as f64;
        scores[i] = (best(&t.circle), best(&t.dash));
    }
    (scores.iter().filter(|(c, d)| c > d).count() as u8, scores)
}

/// 鑑定ボックスの左上のマスの中心、マスの間隔、列と行の数
pub const BOX_ORIGIN: (usize, usize) = (905, 125);
pub const BOX_PITCH: usize = 36;
pub const BOX_COLS: usize = 10;
pub const BOX_ROWS: usize = 7;
/// 黄色い枠の画素の割合がこれ未満なら、枠が写っていないとみなす
pub const INDEX_MIN_SCORE: f64 = 0.1;

/// 鑑定ボックスのマスごとに、枠のあたり (外側 4px) の黄色い画素の割合 (マスの正方形全体に対する割合)
fn box_highlight(screen: &RgbaView) -> Vec<f64> {
    let yellow = color::in_range(&color::hsv(screen), [18, 120, 150], [35, 255, 255]);
    let h = BOX_PITCH / 2;
    let mut out = Vec::new();
    for r in 0..BOX_ROWS {
        for c in 0..BOX_COLS {
            let (cx, cy) = (BOX_ORIGIN.0 + c * BOX_PITCH, BOX_ORIGIN.1 + r * BOX_PITCH);
            let mut n = 0;
            for y in 0..2 * h {
                for x in 0..2 * h {
                    let inner = (4..2 * h - 4).contains(&x) && (4..2 * h - 4).contains(&y);
                    if !inner && yellow.get(cx - h + x, cy - h + y) {
                        n += 1;
                    }
                }
            }
            out.push(n as f64 / (4 * h * h) as f64);
        }
    }
    out
}

/// 何個目か (1 から。枠が写っていなければ None)
fn read_index(screen: &RgbaView) -> Scored<Option<u8>> {
    let yel = box_highlight(screen);
    let mut order: Vec<usize> = (0..yel.len()).collect();
    order.sort_by(|&a, &b| yel[b].partial_cmp(&yel[a]).unwrap());
    // 同じ割合なら、左上のマス (Python 版の argmax と同じく番号の若いほう)
    let i = order[0];
    let value = |i: usize| (yel[i] >= INDEX_MIN_SCORE).then_some(i as u8 + 1);
    Scored {
        value: value(i),
        score: yel[i],
        margin: yel[i] - yel[order[1]],
        candidates: order
            .iter()
            .take(CANDIDATES)
            .map(|&k| (value(k), yel[k]))
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// 画面 1 枚
// ---------------------------------------------------------------------------

/// 位置合わせ後の画面 (1280x720) を読んだ結果 (Python 版の read_screen と同じ中身)
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenRead {
    pub skill1: Scored<SkillId>,
    /// None は第 2 スキルなし (「-----」)。そのとき score と margin は 1
    pub skill2: Scored<Option<SkillId>>,
    pub pts1: Scored<i8>,
    pub pts2: Scored<Option<i8>>,
    /// ポイントごとの (「+」との一致度, 「-」との一致度)
    pub signs: Vec<(f64, f64)>,
    pub slots: u8,
    pub slot_scores: [(f64, f64); 3],
    /// パネルのラベルのずれ (dx, dy) と一致度
    pub panel: (i64, i64, f64),
    pub index: Scored<Option<u8>>,
}

/// トップハット済みの画像で、パネルのラベルを予想位置の ±LABEL_SEARCH px で探し、(dx, dy, 一致度) を返す
fn panel_offset(mx: &Image<u8>, t: &Templates) -> (i64, i64, f64) {
    let (x0, y0, x1, y1) = LABEL_BLOCK;
    let r = LABEL_SEARCH;
    let win = tophat_region(mx, (x0 - r, y0 - r, x1 + r, y1 + r));
    let (v, (x, y)) = max_loc(&match_template(&win, &t.label, None));
    (x as i64 - r as i64, y as i64 - r as i64, v as f64)
}

/// 位置合わせ後の画面 (1280x720 の RGBA) を読む
pub fn read_screen(screen: &RgbaView, t: &Templates) -> ScreenRead {
    let mut mx = color::max_channel(screen);
    let panel = panel_offset(&mx, t);
    if panel.2 >= LABEL_MIN_SCORE {
        // ラベルが確かに見つかったときだけ、そのずれの分だけ画像を戻してから読む
        mx = translate(&mx, panel.0 as i32, panel.1 as i32);
    }
    let roi = |rect: Rect| tophat_region(&mx, rect);
    let left = NAME_LEFT - NAME1.0; // 名前の左端 (欄の座標)
    let names = Some((left - NAME_SEARCH, left + NAME_SEARCH));
    let skill1 = best_match(&roi(NAME1), &t.skills, names, Some(RECHECK_MAX_DIFF));
    let name2 = roi(NAME2);
    let skill2 = if is_dashes(&name2) {
        Scored {
            value: None,
            score: 1.0,
            margin: 1.0,
            candidates: vec![(None, 1.0)],
        }
    } else {
        let s = best_match(&name2, &t.skills, names, Some(RECHECK_MAX_DIFF));
        Scored {
            value: Some(s.value),
            score: s.score,
            margin: s.margin,
            candidates: s.candidates.iter().map(|&(k, v)| (Some(k), v)).collect(),
        }
    };
    let (pts1, h1) = read_points(&roi(PTS1), t);
    let mut signs = vec![h1];
    let pts2 = if skill2.value.is_some() {
        let (p, h2) = read_points(&roi(PTS2), t);
        signs.push(h2);
        Scored {
            value: Some(p.value),
            score: p.score,
            margin: p.margin,
            candidates: p.candidates.iter().map(|&(k, v)| (Some(k), v)).collect(),
        }
    } else {
        Scored {
            value: None,
            score: 1.0,
            margin: 1.0,
            candidates: vec![(None, 1.0)],
        }
    };
    let (slots, slot_scores) = read_slots(&mx, t);
    ScreenRead {
        skill1,
        skill2,
        pts1,
        pts2,
        signs,
        slots,
        slot_scores,
        panel,
        index: read_index(screen),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{json_f32, json_fixture, json_image, json_u8};

    fn sum(img: &Image<u8>) -> i64 {
        img.data().iter().map(|&v| v as i64).sum()
    }

    /// 1.03 倍は、Python 版の OpenCV では Intel IPP が拡大するので、ごく一部の画素が ±1 違う (cv::resize を参照)
    fn assert_scaled(got: &Image<u8>, want: &Image<u8>, scale: usize, what: &str) {
        assert_eq!(
            (got.width(), got.height()),
            (want.width(), want.height()),
            "{what} の大きさ"
        );
        if scale == 2 {
            let diff: Vec<i32> = got
                .data()
                .iter()
                .zip(want.data())
                .map(|(&a, &b)| a as i32 - b as i32)
                .filter(|&d| d != 0)
                .collect();
            assert!(
                diff.iter().all(|d| d.abs() <= 1) && diff.len() * 20 <= got.data().len(),
                "{what}: {diff:?}"
            );
        } else {
            assert_eq!(got, want, "{what}");
        }
    }

    #[test]
    fn templates_match_python() {
        let f = json_fixture("read/templates.json");
        let t = Templates::load();
        for (kind, list) in [
            ("skill", &t.skills),
            (
                "pts",
                &t.digits
                    .iter()
                    .map(|(k, s)| (*k as u16, s.clone()))
                    .collect(),
            ),
        ] {
            let want = f[kind].as_object().unwrap();
            assert_eq!(list.len(), want.len(), "{kind} の数");
            for (k, scaled) in list.iter() {
                for (i, img) in scaled.iter().enumerate() {
                    let w = &want[&k.to_string()][i];
                    let what = format!("{kind}/{k} の {} 倍", TMPL_SCALES[i]);
                    assert_eq!(
                        (img.width() as u64, img.height() as u64),
                        (w[0].as_u64().unwrap(), w[1].as_u64().unwrap()),
                        "{what}"
                    );
                    let tol = if i == 2 {
                        img.data().len() as i64 / 20
                    } else {
                        0
                    };
                    assert!(
                        (sum(img) - w[2].as_i64().unwrap()).abs() <= tol,
                        "{what} の画素の合計"
                    );
                }
            }
        }
        for (key, want) in f["full"].as_object().unwrap() {
            let (kind, k) = key.split_once('/').unwrap();
            let k: u16 = k.parse().unwrap();
            let got = if kind == "skill" {
                &t.skills.iter().find(|(id, _)| *id == k).unwrap().1
            } else {
                &t.digits.iter().find(|(d, _)| *d as u16 == k).unwrap().1
            };
            for i in 0..3 {
                assert_scaled(&got[i], &json_image(&want[i], json_u8), i, key);
            }
        }
        assert_eq!(t.plus, json_image(&f["signs"]["plus"], json_f32));
        assert_eq!(t.minus, json_image(&f["signs"]["minus"], json_f32));
        assert_eq!(t.circle, json_image(&f["slots"]["circle"], json_f32));
        assert_eq!(t.dash, json_image(&f["slots"]["dash"], json_f32));
        assert_eq!(t.label, json_image(&f["label"], json_u8));
    }
}
