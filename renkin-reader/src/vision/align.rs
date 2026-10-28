//! 位置合わせ: 写真を 1280x720 の画面の座標に写す。experimental-charm-reader の align.py を移したもの。
//!
//! 1. 型紙 (どの場面でも同じ不透明な UI だけを残した画像) と SIFT の特徴点を対応づけ、RANSAC で射影変換 H を求める
//! 2. パネルなどの角の黄色い飾り (角飾り) を近い目印に対応づけて、H を解き直す (許容誤差を 10 → 6 → 4px と縮める)
//! 3. 鑑定ボックスのマス目の交点を、画面の座標に写した画像の上で予想位置のまわりから探し、H を解き直す (2 回)
//! 4. 交点が少ない・裏返し・極端に細長い・潰れた H は失敗。失敗したら、1 対 1 の対応づけ、左右反転の順に試し直す
//! 5. 合っても、文字のある所で画面 1px が写真の 0.5px 未満なら、文字が小さすぎるので失敗

use super::assets;
use super::cv::color;
use super::cv::components::connected_components;
use super::cv::image::{Flipped, Image, Rgb, RgbSource};
use super::cv::knn::{DMatch, knn2};
use super::cv::linalg::{Mat3, cond3, inv3, mul3, singular_values2};
use super::cv::matching::{match_template, max_loc};
use super::cv::ransac::{Method, Point, find_homography};
use super::cv::resize::{resize_area, resize_cubic};
use super::cv::sift::{DESCRIPTOR_LEN, KeyPoint, Sift};
use super::cv::warp::{transform_point, warp_perspective, warp_perspective_rect};
use crate::timing::time;
use crate::types::{Diagnostics, RejectReason};

/// 位置合わせ後の画面の大きさ
pub const SCREEN: (usize, usize) = (1280, 720);
/// 写真をこの長辺にそろえてから特徴点を取る (大きい写真は縮め、小さい画像は拡大する)
pub const WORK_LONG_SIDE: f64 = 2000.0;
/// SIFT の特徴点の数の上限
pub const SIFT_FEATURES: usize = 8000;
/// 対応点がこれ未満なら失敗
pub const MIN_MATCHES: usize = 12;

/// 角飾りの中心 (1280x720 で実測): 錬金結果メニュー、装備ボックスの空き、お守りパネル、鑑定ボックスの四隅
const PANELS: [(f32, f32, f32, f32); 4] = [
    (24.4, 54.6, 394.6, 176.4),
    (24.4, 200.6, 394.6, 248.4),
    (470.4, 100.6, 848.6, 306.4),
    (876.4, 54.6, 1254.6, 488.4),
];
/// 当て直しで角飾りを何回分の対応点として入れるか
pub const CORNER_WEIGHT: usize = 5;
/// 鑑定ボックスのマス目の交点 (9 列 x 6 行)
const GRID_COLS: usize = 9;
const GRID_ROWS: usize = 6;
const DOT_HALF: i64 = 8;
/// 交点を予想位置から ±何 px 探すか (交点の間隔 36 の半分未満)
pub const GRID_SEARCH: i64 = 10;
/// これ未満の交点は使わない
pub const GRID_MIN_SCORE: f32 = 0.6;
pub const GRID_WEIGHT: usize = 2;
/// 見つかった交点がこれ未満なら失敗
pub const GRID_MIN_POINTS: usize = 8;
/// 画面 → 写真の局所的な伸びの縦横比 (特異値の比) の上限
pub const MAX_ANISOTROPY: f64 = 4.0;
const SHAPE_POINTS: [[f32; 2]; 5] = [
    [470.0, 100.0],
    [1254.0, 100.0],
    [470.0, 488.0],
    [1254.0, 488.0],
    [640.0, 360.0],
];
/// 読み取る文字のある所で、画面 1px が写真の何 px になるか (一番縮んだ方向) の下限
pub const MIN_RESOLUTION: f64 = 0.5;
const TEXT_POINTS: [[f32; 2]; 4] = [
    [476.0, 166.0],
    [850.0, 166.0],
    [476.0, 247.0],
    [850.0, 247.0],
];

fn corners() -> Vec<Point> {
    PANELS
        .iter()
        .flat_map(|&(x0, y0, x1, y1)| [[x0, y0], [x1, y0], [x0, y1], [x1, y1]])
        .collect()
}

/// 交点の位置 (整数。Python 版の np.round と同じく偶数への丸め)
fn grid() -> Vec<(i64, i64)> {
    (0..GRID_ROWS)
        .flat_map(|r| {
            (0..GRID_COLS).map(move |c| {
                (
                    (922.5 + 36.0 * c as f64).round_ties_even() as i64,
                    (144.0 + 36.0 * r as f64) as i64,
                )
            })
        })
        .collect()
}

/// 位置合わせの結果
pub struct Aligned {
    /// 1280x720 に写した画像
    pub screen: Image<Rgb>,
    /// 渡された写真の座標 → 画面の座標
    pub h: Mat3,
    pub diagnostics: Diagnostics,
}

/// 型紙 (assets/reference.png) の SIFT の特徴点の位置と記述子を、あらかじめ求めて埋め込んだもの。
/// 起動のたびに型紙の SIFT を求めると重い (準備の 9 割) ので、求めた結果だけを持つ。今の SIFT で型紙から求めたものと
/// 同じかは、テストの reference_features_are_up_to_date が確かめる (SIFT や型紙を変えたら、そこで作り直す)。
/// 並び: 個数 (u32、リトルエンディアン)、そのあと 1 個ずつ x、y (f32、リトルエンディアン)、記述子 (128 バイト。
/// SIFT の記述子の値は 0〜255 の整数なので、1 バイトずつで失われない)
static REFERENCE_FEATURES: &[u8] = include_bytes!("../../assets/reference_features.bin");

/// 埋め込んだ型紙の特徴点 (位置と記述子) を取り出す
fn reference_features() -> (Vec<Point>, Vec<[f32; DESCRIPTOR_LEN]>) {
    let n = u32::from_le_bytes(REFERENCE_FEATURES[..4].try_into().unwrap()) as usize;
    let record = 8 + DESCRIPTOR_LEN;
    assert_eq!(
        REFERENCE_FEATURES.len(),
        4 + n * record,
        "型紙の特徴量の大きさが合わない"
    );
    REFERENCE_FEATURES[4..]
        .chunks_exact(record)
        .map(|r| {
            let f = |i: usize| f32::from_le_bytes(r[i..i + 4].try_into().unwrap());
            let des: [f32; DESCRIPTOR_LEN] = std::array::from_fn(|i| f32::from(r[8 + i]));
            ([f(0), f(4)], des)
        })
        .unzip()
}

pub struct Aligner {
    sift: Sift,
    /// 型紙の特徴点の位置 (型紙 = 画面の座標) と記述子
    ref_pts: Vec<Point>,
    ref_des: Vec<[f32; DESCRIPTOR_LEN]>,
    dot: Image<f32>,
}

/// 位置合わせの下ごしらえ (縮小の倍率、縮小した写真、特徴点、型紙との総当たりの対応)
struct Prepared {
    s: f64,
    small: Image<Rgb>,
    kps: Vec<KeyPoint>,
    knn: Vec<Option<(DMatch, DMatch)>>,
}

/// 1 回の試し (そのまま / 1 対 1 / 左右反転) の失敗
struct Failure {
    diagnostics: Diagnostics,
}

/// H が潰れているか (逆が求まらない・値が有限でない)
fn degenerate(h: &Mat3) -> bool {
    !h.iter().flatten().all(|v| v.is_finite()) || cond3(h) > 1e12
}

/// 小数第 2 位に丸める (Python 版の round(x, 2)。判定もこの値で行う)
fn round2(x: f64) -> f64 {
    (x * 100.0).round_ties_even() / 100.0
}

/// 画面の点 p のまわりの 1px の正方形を写真に戻したときの、伸び (2x2) の特異値 (大きい順)
fn local_stretch(hinv: &Mat3, p: [f32; 2]) -> (f64, f64) {
    let q0 = transform_point(hinv, p);
    let q1 = transform_point(hinv, [p[0] + 1.0, p[1]]);
    let q2 = transform_point(hinv, [p[0], p[1] + 1.0]);
    let a = [
        [(q1[0] - q0[0]) as f64, (q2[0] - q0[0]) as f64],
        [(q1[1] - q0[1]) as f64, (q2[1] - q0[1]) as f64],
    ];
    singular_values2(&a)
}

/// 読み取る範囲で、画面が写真上でどれだけ細長く潰れているか (伸びの縦横比の最大)
fn anisotropy(h: &Mat3) -> f64 {
    let Some(hinv) = inv3(h) else {
        return f64::INFINITY;
    };
    SHAPE_POINTS
        .iter()
        .map(|&p| {
            let (a, b) = local_stretch(&hinv, p);
            a / b.max(1e-9)
        })
        .fold(0.0, f64::max)
}

/// 文字のある所で、画面 1px が写真の何 px になるか (伸びの小さいほうの特異値) の最小
fn resolution(h: &Mat3) -> f64 {
    let Some(hinv) = inv3(h) else { return 0.0 };
    TEXT_POINTS
        .iter()
        .map(|&p| local_stretch(&hinv, p).1)
        .fold(f64::INFINITY, f64::min)
}

/// 写真を画面に写す H が裏返しか: 画面中央に対応する写真上の点のまわりの三角形を写し、頂点の回る向きを見る
fn is_mirrored(h: &Mat3) -> bool {
    let Some(hinv) = inv3(h) else { return false };
    let c = transform_point(&hinv, [640.0, 360.0]);
    let a = transform_point(h, c);
    let b = transform_point(h, [c[0] + 10.0, c[1]]);
    let d = transform_point(h, [c[0], c[1] + 10.0]);
    let (u, v) = ([b[0] - a[0], b[1] - a[1]], [d[0] - a[0], d[1] - a[1]]);
    ((u[0] * v[1] - u[1] * v[0]) as f64) < 0.0
}

/// 角飾りの候補 (写真の座標): 黄色で正方形に近い小さな塊をすべて拾う (どれが本物かは位置で決める)
fn corner_candidates(small: &Image<Rgb>) -> Vec<Point> {
    let mask = color::in_range(&color::hsv(small), [12, 50, 140], [38, 200, 255]);
    let (_, comps) = connected_components(&mask);
    comps
        .iter()
        .filter(|c| {
            let (w, h, a) = (c.width as f64, c.height as f64, c.area as f64);
            (6.0..=60.0).contains(&w)
                && (6.0..=60.0).contains(&h)
                && (0.6..=1.6).contains(&(w / h))
                && (0.35..=0.9).contains(&(a / (w * h)))
        })
        .map(|c| [c.cx as f32, c.cy as f32])
        .collect()
}

/// 画面の座標に写した候補 q を、一番近い目印に 1 対 1 で対応づける。[(候補の番号, 目印の番号)]
fn associate(q: &[Point], corners: &[Point], tol: f32) -> Vec<(usize, usize)> {
    let mut best: Vec<(usize, usize, f32)> = Vec::new(); // (目印, 候補, 距離)。目印が最初に出てきた順
    for (i, p) in q.iter().enumerate() {
        let dists: Vec<f32> = corners
            .iter()
            .map(|c| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt())
            .collect();
        let j = (0..dists.len()).fold(0, |m, k| if dists[k] < dists[m] { k } else { m });
        if dists[j] < tol {
            match best.iter_mut().find(|b| b.0 == j) {
                Some(b) if dists[j] < b.2 => (b.1, b.2) = (i, dists[j]),
                Some(_) => {}
                None => best.push((j, i, dists[j])),
            }
        }
    }
    best.iter().map(|&(j, i, _)| (i, j)).collect()
}

/// a の後ろに b を n 回つなげる
fn stack(a: &[Point], b: &[Point], n: usize) -> Vec<Point> {
    let mut v = a.to_vec();
    for _ in 0..n {
        v.extend_from_slice(b);
    }
    v
}

impl Aligner {
    /// 埋め込んだ型紙の特徴点を取り出す
    pub fn new() -> Self {
        let (ref_pts, ref_des) = reference_features();
        Self {
            sift: Sift::new(SIFT_FEATURES),
            ref_pts,
            ref_des,
            dot: assets::GRID_DOT.image().map(f32::from),
        }
    }

    /// 写真を画面に写す。失敗したら (撮り直しの理由, そのままで試したときの途中経過)
    pub fn align(&self, photo: &impl RgbSource) -> Result<Aligned, (RejectReason, Diagnostics)> {
        let mut aligned = self
            .align_any(photo)
            .map_err(|f| (RejectReason::NotAligned, f.diagnostics))?;
        let res = round2(resolution(&aligned.h));
        aligned.diagnostics.resolution = Some(res as f32);
        if res < MIN_RESOLUTION {
            return Err((RejectReason::TooSmall, aligned.diagnostics));
        }
        Ok(aligned)
    }

    /// そのまま → 1 対 1 → 左右反転 (そのまま、1 対 1) の順に試す。
    /// 1 対 1 の試し直しは対応の選び方だけが違うので、縮小・特徴点・総当たりの対応は使い回す
    fn align_any(&self, photo: &impl RgbSource) -> Result<Aligned, Failure> {
        let prep = self.prepare(photo);
        let first = match self.align_once(photo, &prep, false) {
            Ok(a) => return Ok(a),
            Err(f) => f,
        };
        if let Ok(a) = self.align_once(photo, &prep, true) {
            return Ok(a);
        }
        let flipped = Flipped(photo);
        let prep = self.prepare(&flipped);
        let mut a = self
            .align_once(&flipped, &prep, false)
            .or_else(|_| self.align_once(&flipped, &prep, true))
            .map_err(|_| first)?;
        a.diagnostics.mirrored = Some(true);
        // 反転した写真の x は (幅 - 1 - x)。渡された写真の座標 → 反転した写真の座標 → 画面の座標
        let w = photo.width() as f64;
        a.h = mul3(
            &a.h,
            &[[-1.0, 0.0, w - 1.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        );
        Ok(a)
    }

    /// 下ごしらえ: 写真を縮小し、特徴点を取って、型紙の特徴点と総当たりで対応づける
    fn prepare(&self, photo: &impl RgbSource) -> Prepared {
        let s = WORK_LONG_SIDE / photo.width().max(photo.height()) as f64;
        let small = time("align.resize", || {
            if (photo.width() as f64 * s).round_ties_even() as usize == photo.width()
                && (photo.height() as f64 * s).round_ties_even() as usize == photo.height()
            {
                // 大きさが変わらないなら、そのまま (cv2.resize と同じ)
                Image::from_vec(
                    photo.width(),
                    photo.height(),
                    (0..photo.height())
                        .flat_map(|y| (0..photo.width()).map(move |x| (x, y)))
                        .map(|(x, y)| photo.rgb(x, y))
                        .collect(),
                )
            } else if s < 1.0 {
                resize_area(photo, s)
            } else {
                resize_cubic(photo, s, 16)
            }
        });
        let gray = time("align.gray", || color::gray(&small));
        let (kps, des) = self.sift.detect_and_compute(&gray);
        let knn = time("align.match", || knn2(&des, &self.ref_des));
        Prepared { s, small, kps, knn }
    }

    /// 1 回の位置合わせ (反転は試さない)。unique なら、型紙の 1 つの特徴点に写真の特徴点を 1 つだけ対応させる
    fn align_once(
        &self,
        photo: &impl RgbSource,
        prep: &Prepared,
        unique: bool,
    ) -> Result<Aligned, Failure> {
        let mut diag = Diagnostics {
            mirrored: Some(false),
            ..Default::default()
        };
        let fail = |diag: &Diagnostics| Failure {
            diagnostics: diag.clone(),
        };
        let Prepared { s, small, kps, knn } = prep;
        let s = *s;
        let mut good: Vec<DMatch> = knn
            .iter()
            .flatten()
            .filter(|(a, b)| (a.distance as f64) < 0.75 * b.distance as f64)
            .map(|(a, _)| *a)
            .collect();
        if unique {
            // 型紙の 1 つの特徴点には、写真の特徴点を 1 つ (一番近いもの) だけ対応させる。順番は型紙の点が最初に出てきた順
            let mut best: Vec<DMatch> = Vec::new();
            for m in good {
                match best.iter_mut().find(|b| b.train == m.train) {
                    Some(b) if m.distance < b.distance => *b = m,
                    Some(_) => {}
                    None => best.push(m),
                }
            }
            good = best;
        }
        diag.matches = Some(good.len() as u32);
        if good.len() < MIN_MATCHES {
            return Err(fail(&diag));
        }
        let src: Vec<Point> = good
            .iter()
            .map(|m| [kps[m.query].x, kps[m.query].y])
            .collect();
        let dst: Vec<Point> = good.iter().map(|m| self.ref_pts[m.train]).collect();
        let Some((h, inl)) = time("align.ransac", || {
            find_homography(&src, &dst, Method::Ransac, 3.0)
        })
        .filter(|(h, _)| !degenerate(h)) else {
            return Err(fail(&diag));
        };
        let (src, dst): (Vec<Point>, Vec<Point>) = src
            .iter()
            .zip(&dst)
            .zip(&inl)
            .filter(|(_, k)| **k)
            .map(|((a, b), _)| (*a, *b))
            .unzip();
        diag.inliers = Some(src.len() as u32);
        let (h, cs, cd) = time("align.corners", || {
            refine_with_corners(small, h, &src, &dst)
        });
        diag.corners = Some(cs.len() as u32);
        let (h, n_grid) = time("align.grid", || {
            self.refine_with_grid(small, h, &src, &dst, &cs, &cd)
        });
        diag.grid = Some(n_grid as u32);
        if n_grid < GRID_MIN_POINTS || is_mirrored(&h) {
            return Err(fail(&diag));
        }
        let aniso = round2(anisotropy(&h));
        diag.anisotropy = Some(aniso as f32);
        if aniso > MAX_ANISOTROPY {
            return Err(fail(&diag));
        }
        // 縮小前の写真の座標 → 画面の座標
        let h = mul3(&h, &[[s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, 1.0]]);
        let screen = time("align.warp", || {
            warp_perspective(photo, &h, SCREEN.0, SCREEN.1)
        });
        Ok(Aligned {
            screen,
            h,
            diagnostics: diag,
        })
    }

    /// 画面の座標に写した画像で交点を探し、(写真の座標, 画面の座標) の対応を返す
    fn grid_points(&self, small: &Image<Rgb>, h: &Mat3) -> (Vec<Point>, Vec<Point>) {
        // 交点の窓が入る範囲だけを写す (画素ごとに独立なので、画面全体を写して切り出すのと同じ)
        let r = GRID_SEARCH + DOT_HALF;
        let pts = grid();
        let (x0, y0) = (
            pts.iter().map(|p| p.0).min().unwrap() - r,
            pts.iter().map(|p| p.1).min().unwrap() - r,
        );
        let (x1, y1) = (
            pts.iter().map(|p| p.0).max().unwrap() + r,
            pts.iter().map(|p| p.1).max().unwrap() + r,
        );
        let g = color::max_channel(&warp_perspective_rect(
            small,
            h,
            x0 as usize..x1 as usize,
            y0 as usize..y1 as usize,
        ))
        .map(f32::from);
        let (mut found, mut canon) = (Vec::new(), Vec::new());
        for (x, y) in pts {
            let win = g.crop(
                (x - r - x0) as usize,
                (y - r - y0) as usize,
                2 * r as usize,
                2 * r as usize,
            );
            let px: Vec<f64> = win.data().iter().map(|&v| v as f64).collect();
            let mean = px.iter().sum::<f64>() / px.len() as f64;
            let std = (px.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / px.len() as f64).sqrt();
            if std < 3.0 {
                continue; // 写っていない
            }
            let (v, (lx, ly)) = max_loc(&match_template(&win, &self.dot, None));
            if v >= GRID_MIN_SCORE {
                found.push([
                    (x - r + lx as i64 + DOT_HALF) as f32,
                    (y - r + ly as i64 + DOT_HALF) as f32,
                ]);
                canon.push([x as f32, y as f32]);
            }
        }
        let Some(hinv) = inv3(h) else {
            return (Vec::new(), Vec::new());
        };
        (
            found.iter().map(|&p| transform_point(&hinv, p)).collect(),
            canon,
        )
    }

    /// 交点を予想位置の近くで探し、対応点に加えて当て直す (2 回)。(H, 見つかった交点の数)
    fn refine_with_grid(
        &self,
        small: &Image<Rgb>,
        mut h: Mat3,
        src: &[Point],
        dst: &[Point],
        cs: &[Point],
        cd: &[Point],
    ) -> (Mat3, usize) {
        let mut n = 0;
        for _ in 0..2 {
            let (gs, gd) = self.grid_points(small, &h);
            if gs.len() < 6 {
                n = gs.len();
                break;
            }
            let s = stack(&stack(src, cs, CORNER_WEIGHT), &gs, GRID_WEIGHT);
            let d = stack(&stack(dst, cd, CORNER_WEIGHT), &gd, GRID_WEIGHT);
            let Some((h2, _)) =
                find_homography(&s, &d, Method::LeastSquares, 3.0).filter(|(h, _)| !degenerate(h))
            else {
                break;
            };
            (h, n) = (h2, gs.len());
        }
        (h, n)
    }
}

impl Default for Aligner {
    fn default() -> Self {
        Self::new()
    }
}

/// 角飾りを対応点に加えて当て直す (許容誤差を 10 → 6 → 4px と縮める)。(H, 角飾りの写真の座標, 画面の座標)
fn refine_with_corners(
    small: &Image<Rgb>,
    mut h: Mat3,
    src: &[Point],
    dst: &[Point],
) -> (Mat3, Vec<Point>, Vec<Point>) {
    let cands = corner_candidates(small);
    let corners = corners();
    let (mut cs, mut cd) = (Vec::new(), Vec::new());
    for tol in [10.0, 6.0, 4.0] {
        if cands.is_empty() {
            break;
        }
        let q: Vec<Point> = cands.iter().map(|&p| transform_point(&h, p)).collect();
        let pairs = associate(&q, &corners, tol);
        if pairs.is_empty() {
            break;
        }
        let ps: Vec<Point> = pairs.iter().map(|&(i, _)| cands[i]).collect();
        let pd: Vec<Point> = pairs.iter().map(|&(_, j)| corners[j]).collect();
        let Some((h2, _)) = find_homography(
            &stack(src, &ps, CORNER_WEIGHT),
            &stack(dst, &pd, CORNER_WEIGHT),
            Method::LeastSquares,
            3.0,
        )
        .filter(|(h, _)| !degenerate(h)) else {
            break;
        };
        (h, cs, cd) = (h2, ps, pd);
    }
    (h, cs, cd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::load_png_rgba;

    /// 埋め込んだ型紙の特徴量 (REFERENCE_FEATURES) が、今の SIFT で型紙から求めたものとバイト単位で同じか。
    /// SIFT や型紙を変えたら、`RENKIN_WRITE_REFERENCE=1 cargo test --release reference_features_are_up_to_date`
    /// で assets/reference_features.bin を作り直す
    #[test]
    fn reference_features_are_up_to_date() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        let (rgba, w, h) = load_png_rgba(&dir.join("reference.png"));
        let gray = Image::from_vec(
            w as usize,
            h as usize,
            rgba.chunks(4).map(|p| p[0]).collect(),
        );
        let (kps, des) = Sift::new(SIFT_FEATURES).detect_and_compute(&gray);
        let mut bytes = (kps.len() as u32).to_le_bytes().to_vec();
        for (k, d) in kps.iter().zip(&des) {
            bytes.extend(k.x.to_le_bytes());
            bytes.extend(k.y.to_le_bytes());
            for &v in d {
                assert!(
                    v.fract() == 0.0 && (0.0..=255.0).contains(&v),
                    "記述子の値が 0〜255 の整数でない: {v}"
                );
                bytes.push(v as u8);
            }
        }
        if std::env::var("RENKIN_WRITE_REFERENCE").is_ok() {
            std::fs::write(dir.join("reference_features.bin"), &bytes).unwrap();
            println!(
                "assets/reference_features.bin を書き出した ({} 個、{} バイト)",
                kps.len(),
                bytes.len()
            );
            return;
        }
        assert!(
            bytes == REFERENCE_FEATURES,
            "埋め込んだ型紙の特徴量が、今の SIFT の結果と違う。RENKIN_WRITE_REFERENCE=1 を付けてこのテストを流し、作り直す"
        );
    }
}
