//! 対応点から射影変換 (ホモグラフィ) を求める: RANSAC と、全部の点での最小二乗 (どちらも最後に Levenberg–Marquardt で詰める)。
//!
//! OpenCV (Apache License 2.0) の findHomography (modules/geometry/src/fundam.cpp、ptsetreg.cpp、levmarq.cpp、
//! precomp.hpp) を NumPy でなぞった experimental-charm-reader の pure_ransac.py を、Rust に移したもの。
//! 乱数 (cv::RNG) の種も同じなので、選ばれる 4 点の組も同じになる。
//! ライセンスと著作権表示はリポジトリの THIRD_PARTY_LICENSES/OpenCV を参照

use super::linalg::{Mat3, det3, mul3, solve_symmetric, symmetric_eigen};

const FLT_EPSILON: f64 = f32::EPSILON as f64;
const DBL_EPSILON: f64 = f64::EPSILON;
const DBL_MIN: f64 = f64::MIN_POSITIVE;

/// 点 (x, y)
pub type Point = [f32; 2];

/// cv::RNG (multiply-with-carry)。RANSACPointSetRegistrator と同じ種から始める
struct Rng(u64);

impl Rng {
    fn new() -> Self {
        Rng(u64::MAX)
    }

    fn next(&mut self) -> u32 {
        let s = self.0;
        self.0 = (s & 0xFFFF_FFFF)
            .wrapping_mul(4_164_903_690)
            .wrapping_add(s >> 32);
        self.0 as u32
    }

    fn uniform(&mut self, a: u32, b: u32) -> u32 {
        if a == b { a } else { self.next() % (b - a) + a }
    }
}

/// 最後の点が、ほかの 2 点と一直線に並ぶか (precomp.hpp の haveCollinearPoints。最後の点だけを調べる)
fn have_collinear_points(p: &[Point]) -> bool {
    let i = p.len() - 1;
    for j in 0..i {
        let dx1 = p[j][0] as f64 - p[i][0] as f64;
        let dy1 = p[j][1] as f64 - p[i][1] as f64;
        for k in 0..j {
            let dx2 = p[k][0] as f64 - p[i][0] as f64;
            let dy2 = p[k][1] as f64 - p[i][1] as f64;
            if (dx2 * dy1 - dy2 * dx1).abs()
                <= FLT_EPSILON * (dx1.abs() + dy1.abs() + dx2.abs() + dy2.abs())
            {
                return true;
            }
        }
    }
    false
}

/// 4 点の組を使ってよいか: 一直線に並ぶ点が無く、写す前と後で 3 点の回る向きが全部そろっている
fn check_subset(ms1: &[Point], ms2: &[Point]) -> bool {
    if have_collinear_points(ms1) || have_collinear_points(ms2) {
        return false;
    }
    let det = |p: &[Point], t: [usize; 3]| {
        let m: Mat3 = [0, 1, 2].map(|r| [p[t[r]][0] as f64, p[t[r]][1] as f64, 1.0]);
        det3(&m)
    };
    let negative = [[0, 1, 2], [1, 2, 3], [0, 2, 3], [0, 1, 3]]
        .iter()
        .filter(|&&t| det(ms1, t) * det(ms2, t) < 0.0)
        .count();
    negative == 0 || negative == 4
}

fn scale_for(v: f64) -> f64 {
    if v.abs() > FLT_EPSILON { 1.0 / v } else { 1.0 }
}

/// 正規化した DLT: L^T L (9x9) の一番小さい固有値の固有ベクトルから H を作る (H[2][2] = 1)。求まらなければ None
fn run_kernel(src: &[Point], dst: &[Point]) -> Option<Mat3> {
    let n = src.len() as f64;
    let mean = |p: &[Point], k: usize| p.iter().map(|q| q[k] as f64).sum::<f64>() / n;
    let (c_m, c_d) = ([mean(src, 0), mean(src, 1)], [mean(dst, 0), mean(dst, 1)]);
    let spread = |p: &[Point], c: [f64; 2], k: usize| {
        p.iter().map(|q| (q[k] as f64 - c[k]).abs()).sum::<f64>()
    };
    let (s_m, s_d) = (
        [spread(src, c_m, 0), spread(src, c_m, 1)],
        [spread(dst, c_d, 0), spread(dst, c_d, 1)],
    );
    if s_d.iter().chain(&s_m).any(|v| v.abs() < DBL_EPSILON) {
        return None;
    }
    let (s_d, s_m) = ([n / s_d[0], n / s_d[1]], [n / s_m[0], n / s_m[1]]);
    let inv_hnorm: Mat3 = [
        [1.0 / s_d[0], 0.0, c_d[0]],
        [0.0, 1.0 / s_d[1], c_d[1]],
        [0.0, 0.0, 1.0],
    ];
    let hnorm2: Mat3 = [
        [s_m[0], 0.0, -c_m[0] * s_m[0]],
        [0.0, s_m[1], -c_m[1] * s_m[1]],
        [0.0, 0.0, 1.0],
    ];
    let mut ltl = [0.0; 81];
    for (p, q) in src.iter().zip(dst) {
        let (bx, by) = (
            (p[0] as f64 - c_m[0]) * s_m[0],
            (p[1] as f64 - c_m[1]) * s_m[1],
        );
        let (x, y) = (
            (q[0] as f64 - c_d[0]) * s_d[0],
            (q[1] as f64 - c_d[1]) * s_d[1],
        );
        let lx = [bx, by, 1.0, 0.0, 0.0, 0.0, -x * bx, -x * by, -x];
        let ly = [0.0, 0.0, 0.0, bx, by, 1.0, -y * bx, -y * by, -y];
        for i in 0..9 {
            for j in 0..9 {
                ltl[i * 9 + j] += lx[i] * lx[j] + ly[i] * ly[j];
            }
        }
    }
    let (_, vecs) = symmetric_eigen(&ltl, 9);
    let v = &vecs[0];
    let h0: Mat3 = [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]];
    let h = mul3(&mul3(&inv_hnorm, &h0), &hnorm2);
    let s = scale_for(h[2][2]);
    Some(h.map(|r| r.map(|x| x * s)))
}

/// 写した位置のずれの 2 乗 (OpenCV と同じく float32 で計算する)
fn compute_error(src: &[Point], dst: &[Point], h: &Mat3) -> Vec<f32> {
    let hf: Vec<f32> = h.iter().flatten().map(|&v| v as f32).collect();
    src.iter()
        .zip(dst)
        .map(|(p, q)| {
            let ww = 1.0 / (hf[6] * p[0] + hf[7] * p[1] + hf[8]);
            let dx = (hf[0] * p[0] + hf[1] * p[1] + hf[2]) * ww - q[0];
            let dy = (hf[3] * p[0] + hf[4] * p[1] + hf[5]) * ww - q[1];
            dx * dx + dy * dy
        })
        .collect()
}

fn ransac_update_num_iters(p: f64, ep: f64, model_points: i32, max_iters: usize) -> usize {
    let (p, ep) = (p.clamp(0.0, 1.0), ep.clamp(0.0, 1.0));
    let num = (1.0 - p).max(DBL_MIN);
    let denom = 1.0 - (1.0 - ep).powi(model_points);
    if denom < DBL_MIN {
        return 0;
    }
    let (num, denom) = (num.ln(), denom.ln());
    if denom >= 0.0 || -num >= max_iters as f64 * -denom {
        return max_iters;
    }
    (num / denom).round_ties_even() as usize
}

/// 4 点の組をランダムに選ぶ (同じ点は選ばない。使えない組なら選び直す)
fn get_subset(src: &[Point], dst: &[Point], rng: &mut Rng) -> Option<([Point; 4], [Point; 4])> {
    let count = src.len() as u32;
    for _ in 0..10000 {
        let mut idx: Vec<usize> = Vec::with_capacity(4);
        for _ in 0..4 {
            let mut k = rng.uniform(0, count) as usize;
            while idx.contains(&k) {
                k = rng.uniform(0, count) as usize;
            }
            idx.push(k);
        }
        let ms1 = [0, 1, 2, 3].map(|i| src[idx[i]]);
        let ms2 = [0, 1, 2, 3].map(|i| dst[idx[i]]);
        if check_subset(&ms1, &ms2) {
            return Some((ms1, ms2));
        }
    }
    None
}

fn ransac(
    src: &[Point],
    dst: &[Point],
    threshold: f64,
    confidence: f64,
    max_iters: usize,
) -> Option<(Mat3, Vec<bool>)> {
    let count = src.len();
    let mut rng = Rng::new();
    let mut niters = max_iters.max(1);
    let t = (threshold * threshold) as f32;
    let mut best: Option<(Mat3, Vec<bool>)> = None;
    let mut max_good = 0;
    let mut it = 0;
    while it < niters {
        let Some((ms1, ms2)) = get_subset(src, dst, &mut rng) else {
            if it == 0 {
                return None;
            }
            break;
        };
        if let Some(model) = run_kernel(&ms1, &ms2) {
            let mask: Vec<bool> = compute_error(src, dst, &model)
                .iter()
                .map(|&e| e <= t)
                .collect();
            let good = mask.iter().filter(|&&b| b).count();
            if good > max_good.max(3) {
                max_good = good;
                niters = ransac_update_num_iters(
                    confidence,
                    (count - good) as f64 / count as f64,
                    4,
                    niters,
                );
                best = Some((model, mask));
            }
        }
        it += 1;
    }
    best.filter(|_| max_good > 0)
}

/// h (9 個) で写したずれ (点ごとに x, y) と、必要ならヤコビ行列 (2n x 9、行ごと)
fn residuals(
    h: &[f64; 9],
    src: &[Point],
    dst: &[Point],
    need_jac: bool,
) -> (Vec<f64>, Vec<[f64; 9]>) {
    let mut err = Vec::with_capacity(2 * src.len());
    let mut jac = Vec::new();
    for (p, q) in src.iter().zip(dst) {
        let (mx, my) = (p[0] as f64, p[1] as f64);
        let ww = h[6] * mx + h[7] * my + h[8];
        let ww = if ww.abs() > DBL_EPSILON {
            1.0 / ww
        } else {
            0.0
        };
        let xi = (h[0] * mx + h[1] * my + h[2]) * ww;
        let yi = (h[3] * mx + h[4] * my + h[5]) * ww;
        err.push(xi - q[0] as f64);
        err.push(yi - q[1] as f64);
        if need_jac {
            jac.push([
                mx * ww,
                my * ww,
                ww,
                0.0,
                0.0,
                0.0,
                -mx * ww * xi,
                -my * ww * xi,
                -ww * xi,
            ]);
            jac.push([
                0.0,
                0.0,
                0.0,
                mx * ww,
                my * ww,
                ww,
                -mx * ww * yi,
                -my * ww * yi,
                -ww * yi,
            ]);
        }
    }
    (err, jac)
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// JᵀJ (9x9) と Jᵀb
fn normal_eq(jac: &[[f64; 9]], b: &[f64]) -> ([f64; 81], [f64; 9]) {
    let mut jtj = [0.0; 81];
    let mut jtb = [0.0; 9];
    for (row, &bi) in jac.iter().zip(b) {
        for i in 0..9 {
            jtb[i] += row[i] * bi;
            for j in 0..9 {
                jtj[i * 9 + j] += row[i] * row[j];
            }
        }
    }
    (jtj, jtb)
}

/// detail::LevMarqBase::optimize (既定の設定。最大 10 回、測地線の加速あり)
fn levmarq(x0: [f64; 9], src: &[Point], dst: &[Point]) -> [f64; 9] {
    const MAX_ITERS: usize = 10;
    let (h_geo, geo_scale) = (1e-4, 0.5);
    let (min_diag, max_diag, max_lambda) = (1e-6, 1e32, 1e32);
    let mut cur = x0;
    let mut probe = cur;
    let (b, jac) = residuals(&probe, src, dst, true);
    let mut energy = dot(&b, &b);
    let (mut jtj, mut jtb) = normal_eq(&jac, &b);
    let mut jac = jac;
    let mut old_energy = energy;
    let (mut up, mut lambda) = (2.0, 1e-4);
    let mut it = 0;
    let mut done = false;
    while !done {
        let grad_max = jtb.iter().fold(0f64, |m, v| m.max(v.abs()));
        let diag: Vec<f64> = (0..9).map(|i| jtj[i * 9 + i]).collect();
        let mut enough = false;
        while !enough && !done {
            let lm_diag: Vec<f64> = diag
                .iter()
                .map(|d| (d * lambda).clamp(min_diag, max_diag))
                .collect();
            let mut a = jtj;
            for i in 0..9 {
                a[i * 9 + i] = lm_diag[i] + diag[i];
            }
            let neg_jtb: Vec<f64> = jtb.iter().map(|v| -v).collect();
            let mut x = solve_symmetric(&a, &neg_jtb);
            let jac_cost = -0.5
                * (0..9)
                    .map(|i| x[i] * (jtb[i] - lm_diag[i] * x[i]))
                    .sum::<f64>();
            let x_norm = dot(&x, &x);
            // 測地線の加速
            let probe_geo: [f64; 9] = std::array::from_fn(|i| cur[i] + x[i] * h_geo);
            let (bv, _) = residuals(&probe_geo, src, dst, false);
            let mut jtbv = [0.0; 9];
            for (row, &bi) in jac.iter().zip(&bv) {
                for i in 0..9 {
                    jtbv[i] += row[i] * bi;
                }
            }
            let jtrvv: Vec<f64> = (0..9)
                .map(|i| {
                    -((jtbv[i] + jtb[i] * (h_geo - 1.0) + lm_diag[i] * x[i] * h_geo)
                        / (h_geo * h_geo))
                })
                .collect();
            let xgeo = solve_symmetric(&a, &jtrvv);
            if (dot(&xgeo, &xgeo) / dot(&x, &x)).sqrt() < 1.0 {
                for i in 0..9 {
                    x[i] += xgeo[i] * geo_scale;
                }
            }
            probe = std::array::from_fn(|i| cur[i] + x[i]);
            let (b, _) = residuals(&probe, src, dst, false);
            energy = dot(&b, &b);
            let cost_change = old_energy - energy;
            let quality = cost_change / jac_cost;
            let (mut small_grad, mut small_step, mut small_delta) = (false, false, false);
            if cost_change < 0.0 {
                lambda *= up;
                up *= 2.0;
            } else {
                enough = true;
                lambda *= (1.0f64 / 3.0).max(1.0 - (2.0 * quality - 1.0).powi(3));
                up = 2.0;
                small_grad = grad_max < 1e-6;
                small_step = x_norm < 1e-6;
                small_delta = cost_change / energy < 1e-6;
                cur = probe;
                old_energy = energy;
            }
            it += 1;
            done =
                it >= MAX_ITERS || lambda >= max_lambda || small_grad || small_step || small_delta;
        }
        if !done {
            let (b, j) = residuals(&probe, src, dst, true);
            (jtj, jtb) = normal_eq(&j, &b);
            jac = j;
        }
    }
    cur
}

/// 射影変換の求め方
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// 全部の点で最小二乗 (外れ値は除かない)。cv2.findHomography(method=0)
    LeastSquares,
    /// RANSAC で外れ値を除いてから、残った点で最小二乗。cv2.findHomography(method=RANSAC)
    Ransac,
}

/// 対応点 src → dst の射影変換と、ずれがしきい値 (3px) 以内の点の印を返す (cv2.findHomography と同じ)。
/// 求まらなければ None。点が 4 つ未満でも None
pub fn find_homography(
    src: &[Point],
    dst: &[Point],
    method: Method,
    threshold: f64,
) -> Option<(Mat3, Vec<bool>)> {
    let n = src.len();
    if n < 4 {
        return None;
    }
    let (mut h, mut mask) = if method == Method::LeastSquares || n == 4 {
        (run_kernel(src, dst)?, vec![true; n])
    } else {
        ransac(src, dst, threshold, 0.995, 2000)?
    };
    if n > 4 {
        let (s, d): (Vec<Point>, Vec<Point>) = src
            .iter()
            .zip(dst)
            .zip(&mask)
            .filter(|(_, m)| **m)
            .map(|((&a, &b), _)| (a, b))
            .unzip();
        if !s.is_empty() {
            if method == Method::Ransac {
                h = run_kernel(&s, &d)?;
            }
            let x0: [f64; 9] = std::array::from_fn(|i| h[i / 3][i % 3]);
            let x = levmarq(x0, &s, &d);
            let scale = scale_for(x[8]);
            h = std::array::from_fn(|r| std::array::from_fn(|c| x[r * 3 + c] * scale));
            let thr = (threshold * threshold) as f32;
            mask = compute_error(src, dst, &h)
                .iter()
                .map(|&e| e <= thr)
                .collect();
        }
    }
    Some((h, mask))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::json_fixture;

    fn points(v: &serde_json::Value) -> Vec<Point> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|p| [p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32])
            .collect()
    }

    fn mat(v: &serde_json::Value) -> Option<Mat3> {
        if v.is_null() {
            return None;
        }
        Some(std::array::from_fn(|r| {
            std::array::from_fn(|c| v[r][c].as_f64().unwrap())
        }))
    }

    /// 写真の画面の四隅を写したときの、2 つの H のずれの最大 (px)
    fn corner_gap(a: &Mat3, b: &Mat3) -> f64 {
        let apply = |h: &Mat3, x: f64, y: f64| {
            let w = h[2][0] * x + h[2][1] * y + h[2][2];
            (
                (h[0][0] * x + h[0][1] * y + h[0][2]) / w,
                (h[1][0] * x + h[1][1] * y + h[1][2]) / w,
            )
        };
        [
            (0.0, 0.0),
            (2000.0, 0.0),
            (0.0, 1500.0),
            (2000.0, 1500.0),
            (1000.0, 750.0),
        ]
        .iter()
        .map(|&(x, y)| {
            let (p, q) = (apply(a, x, y), apply(b, x, y));
            ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt()
        })
        .fold(0f64, f64::max)
    }

    /// 人工の写真 4 枚 (tests/synthetic/) の対応点で、NumPy 版 (pure_ransac.py) と比べる。
    /// 乱数の進め方まで合わせてあるので、インライアの判定まで同じになる
    #[test]
    fn matches_numpy() {
        let golden = json_fixture("cv/ransac.json");
        for case in golden.as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let (src, dst) = (points(&case["src"]), points(&case["dst"]));
            let got = find_homography(&src, &dst, Method::Ransac, 3.0);
            let want_mask: Vec<bool> = case["mask"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() != 0)
                .collect();
            match (got, mat(&case["H"])) {
                (Some((h, mask)), Some(want)) => {
                    let diff = mask.iter().zip(&want_mask).filter(|(a, b)| a != b).count();
                    println!(
                        "{name}: RANSAC の H のずれ {:.2e} px、インライアの判定の違い {diff} 点",
                        corner_gap(&h, &want)
                    );
                    assert!(corner_gap(&h, &want) < 1e-3 && diff == 0, "{name}");
                    let keep: Vec<usize> = (0..src.len()).filter(|&i| mask[i]).collect();
                    let (s, d): (Vec<Point>, Vec<Point>) =
                        keep.iter().map(|&i| (src[i], dst[i])).unzip();
                    match (
                        find_homography(&s, &d, Method::LeastSquares, 3.0),
                        mat(&case["H_ls"]),
                    ) {
                        (Some((h, _)), Some(want)) => {
                            println!(
                                "{name}: 最小二乗の H のずれ {:.2e} px",
                                corner_gap(&h, &want)
                            );
                            assert!(corner_gap(&h, &want) < 1e-3, "{name} 最小二乗");
                        }
                        (got, want) => {
                            assert_eq!(got.is_some(), want.is_some(), "{name} 最小二乗が求まるか")
                        }
                    }
                }
                (got, want) => {
                    println!("{name}: Rust {} / NumPy {}", got.is_some(), want.is_some());
                    assert_eq!(got.is_some(), want.is_some(), "{name}: 求まるか");
                }
            }
        }
    }
}
