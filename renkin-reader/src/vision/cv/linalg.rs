//! 小さな線形代数 (3x3 の逆行列・連立方程式、2x2 の特異値、対称行列の固有値と固有ベクトル)。
//! 位置合わせの射影変換 (3x3) と、RANSAC の 4 点からの射影変換 (9x9)・最後の詰め (9x9) で使う

/// 3x3 の行列 (行ごと)
pub type Mat3 = [[f64; 3]; 3];

pub fn mul3(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut c = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            c[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    c
}

pub fn det3(a: &Mat3) -> f64 {
    a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
}

/// 逆行列。行列式が 0 なら None
pub fn inv3(a: &Mat3) -> Option<Mat3> {
    let d = det3(a);
    if d == 0.0 || !d.is_finite() {
        return None;
    }
    let c =
        |r0: usize, r1: usize, c0: usize, c1: usize| a[r0][c0] * a[r1][c1] - a[r0][c1] * a[r1][c0];
    Some([
        [c(1, 2, 1, 2) / d, -c(0, 2, 1, 2) / d, c(0, 1, 1, 2) / d],
        [-c(1, 2, 0, 2) / d, c(0, 2, 0, 2) / d, -c(0, 1, 0, 2) / d],
        [c(1, 2, 0, 1) / d, -c(0, 2, 0, 1) / d, c(0, 1, 0, 1) / d],
    ])
}

/// a x = b を解く (部分ピボットつきのガウスの消去法)。解けなければ None
pub fn solve3(a: &Mat3, b: &[f64; 3]) -> Option<[f64; 3]> {
    let mut m = [
        [a[0][0], a[0][1], a[0][2], b[0]],
        [a[1][0], a[1][1], a[1][2], b[1]],
        [a[2][0], a[2][1], a[2][2], b[2]],
    ];
    for col in 0..3 {
        let p = (col..3)
            .max_by(|&i, &j| m[i][col].abs().total_cmp(&m[j][col].abs()))
            .unwrap();
        if m[p][col] == 0.0 {
            return None;
        }
        m.swap(col, p);
        for r in col + 1..3 {
            let f = m[r][col] / m[col][col];
            let pivot = m[col];
            for (v, p) in m[r].iter_mut().zip(pivot).skip(col) {
                *v -= f * p;
            }
        }
    }
    let mut x = [0.0; 3];
    for r in (0..3).rev() {
        x[r] = (m[r][3] - (r + 1..3).map(|k| m[r][k] * x[k]).sum::<f64>()) / m[r][r];
    }
    Some(x)
}

/// 2x2 の行列の特異値 (大きい順)
pub fn singular_values2(a: &[[f64; 2]; 2]) -> (f64, f64) {
    // AᵀA の固有値の平方根
    let p = a[0][0] * a[0][0] + a[1][0] * a[1][0];
    let q = a[0][1] * a[0][1] + a[1][1] * a[1][1];
    let r = a[0][0] * a[0][1] + a[1][0] * a[1][1];
    let mean = (p + q) / 2.0;
    let d = (((p - q) / 2.0).powi(2) + r * r).sqrt();
    ((mean + d).max(0.0).sqrt(), (mean - d).max(0.0).sqrt())
}

/// n x n の対称行列 a (行ごとに並べたもの) の固有値 (小さい順) と、対応する固有ベクトル (vecs[i] が i 番目)。
/// ヤコビ法 (非対角成分を 1 つずつ回転で 0 にしていく)
pub fn symmetric_eigen(a: &[f64], n: usize) -> (Vec<f64>, Vec<Vec<f64>>) {
    let mut m = a.to_vec();
    let mut v = vec![0.0; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    for _sweep in 0..100 {
        let off: f64 = (0..n)
            .flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| m[i * n + j].powi(2))
            .sum();
        if off < 1e-300 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = m[p * n + q];
                if apq == 0.0 {
                    continue;
                }
                let theta = (m[q * n + q] - m[p * n + p]) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let t = if theta == 0.0 { 1.0 } else { t };
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for k in 0..n {
                    let (mkp, mkq) = (m[k * n + p], m[k * n + q]);
                    m[k * n + p] = c * mkp - s * mkq;
                    m[k * n + q] = s * mkp + c * mkq;
                }
                for k in 0..n {
                    let (mpk, mqk) = (m[p * n + k], m[q * n + k]);
                    m[p * n + k] = c * mpk - s * mqk;
                    m[q * n + k] = s * mpk + c * mqk;
                }
                for k in 0..n {
                    let (vkp, vkq) = (v[k * n + p], v[k * n + q]);
                    v[k * n + p] = c * vkp - s * vkq;
                    v[k * n + q] = s * vkp + c * vkq;
                }
            }
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| m[i * n + i].total_cmp(&m[j * n + j]));
    let vals = order.iter().map(|&i| m[i * n + i]).collect();
    let vecs = order
        .iter()
        .map(|&i| (0..n).map(|k| v[k * n + i]).collect())
        .collect();
    (vals, vecs)
}

/// 3x3 の行列の条件数 (一番大きい特異値 / 一番小さい特異値)。np.linalg.cond と同じ
pub fn cond3(a: &Mat3) -> f64 {
    let mut ata = [0.0; 9];
    for i in 0..3 {
        for j in 0..3 {
            ata[i * 3 + j] = (0..3).map(|k| a[k][i] * a[k][j]).sum();
        }
    }
    let (vals, _) = symmetric_eigen(&ata, 3);
    (vals[2].max(0.0) / vals[0].max(0.0)).sqrt()
}

/// 対称で半正定値の行列 a (n x n) について a x = b を解く。特異値 (固有値の絶対値) が、その合計 x 2 x DBL_EPSILON 以下の
/// 成分は捨てる (OpenCV の cv::solve(..., DECOMP_SVD) と同じ扱い)
pub fn solve_symmetric(a: &[f64], b: &[f64]) -> Vec<f64> {
    let n = b.len();
    let (vals, vecs) = symmetric_eigen(a, n);
    let thr = vals.iter().map(|w| w.abs()).sum::<f64>() * 2.0 * f64::EPSILON;
    let mut x = vec![0.0; n];
    for (w, v) in vals.iter().zip(&vecs) {
        if w.abs() <= thr {
            continue;
        }
        let coef = v.iter().zip(b).map(|(vi, bi)| vi * bi).sum::<f64>() / w;
        for k in 0..n {
            x[k] += coef * v[k];
        }
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::json_fixture;

    fn m3(v: &serde_json::Value) -> Mat3 {
        let mut a = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] = v[i][j].as_f64().unwrap();
            }
        }
        a
    }

    fn vec(v: &serde_json::Value) -> Vec<f64> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect()
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + b.abs())
    }

    #[test]
    fn matches_numpy() {
        let f = json_fixture("cv/linalg.json");
        let a3 = m3(&f["a3"]);
        let inv = inv3(&a3).unwrap();
        let want = m3(&f["inv3"]);
        assert!(
            (0..3).all(|i| (0..3).all(|j| close(inv[i][j], want[i][j], 1e-12))),
            "逆行列"
        );
        let b = vec(&f["b3"]);
        let x = solve3(&a3, &[b[0], b[1], b[2]]).unwrap();
        assert!(
            x.iter()
                .zip(vec(&f["solve3"]))
                .all(|(a, b)| close(*a, b, 1e-12)),
            "連立方程式"
        );
        assert!(
            close(det3(&a3), f["det3"].as_f64().unwrap(), 1e-12),
            "行列式"
        );
        let a2 = [
            [
                f["a2"][0][0].as_f64().unwrap(),
                f["a2"][0][1].as_f64().unwrap(),
            ],
            [
                f["a2"][1][0].as_f64().unwrap(),
                f["a2"][1][1].as_f64().unwrap(),
            ],
        ];
        let (s0, s1) = singular_values2(&a2);
        let sv = vec(&f["sv2"]);
        assert!(
            close(s0, sv[0], 1e-12) && close(s1, sv[1], 1e-12),
            "2x2 の特異値"
        );
        let sym: Vec<f64> = f["sym9"].as_array().unwrap().iter().flat_map(vec).collect();
        let (vals, vecs) = symmetric_eigen(&sym, 9);
        assert!(
            vals.iter()
                .zip(vec(&f["eigvals9"]))
                .all(|(a, b)| close(*a, b, 1e-10)),
            "固有値"
        );
        // 固有ベクトルは向き (符号) が決まらないので、そろえて比べる
        let want = vec(&f["eigvec9_min"]);
        let sign = if vecs[0].iter().zip(&want).map(|(a, b)| a * b).sum::<f64>() < 0.0 {
            -1.0
        } else {
            1.0
        };
        assert!(
            vecs[0]
                .iter()
                .zip(&want)
                .all(|(a, b)| close(a * sign, *b, 1e-9)),
            "一番小さい固有値の固有ベクトル"
        );
        assert!(
            close(cond3(&m3(&f["h"])), f["cond_h"].as_f64().unwrap(), 1e-9),
            "条件数"
        );
        let rhs: Vec<f64> = vec(&f["b3"]).repeat(3);
        let x = solve_symmetric(&sym, &rhs);
        assert!(
            x.iter()
                .zip(vec(&f["svd_solve"]["x"]))
                .all(|(a, b)| close(*a, b, 1e-9)),
            "対称行列の連立方程式"
        );
    }
}
