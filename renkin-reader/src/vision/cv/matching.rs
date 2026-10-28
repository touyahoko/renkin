//! 照合 (正規化相互相関) と相関係数
//!
//! OpenCV (Apache License 2.0) の modules/imgproc/src/templmatch.cpp の TM_CCOEFF_NORMED (分散がほぼ 0 の窓の扱い) に合わせて、Rust で書いたもの。
//! ライセンスと著作権表示はリポジトリの THIRD_PARTY_LICENSES/OpenCV を参照

use std::ops::Range;

use super::image::Image;

/// 見本 templ を src の上でずらしながら、各位置の正規化相互相関 (平均を引いてから、OpenCV の TM_CCOEFF_NORMED) を返す。
/// cols を渡すと、見本の左端がその範囲に来る位置だけを計算する (結果の 0 列目が cols.start)。
/// 窓の明るさが一定 (分散 0) で計算できない位置は、OpenCV と同じく 0 にする
pub fn match_template<T: Copy + Into<f64>>(
    src: &Image<T>,
    templ: &Image<T>,
    cols: Option<Range<usize>>,
) -> Image<f32> {
    let (sw, sh, tw, th) = (src.width(), src.height(), templ.width(), templ.height());
    assert!(tw <= sw && th <= sh, "見本が照合する画像より大きい");
    let all = 0..sw - tw + 1;
    let cols = cols.map_or(all.clone(), |c| c.start.max(all.start)..c.end.min(all.end));
    let n = (tw * th) as f64;
    let t: Vec<f64> = templ.data().iter().map(|&v| v.into()).collect();
    let t_mean = t.iter().sum::<f64>() / n;
    let t: Vec<f64> = t.iter().map(|v| v - t_mean).collect();
    let t_norm = t.iter().map(|v| v * v).sum::<f64>().sqrt();
    let s: Vec<f64> = src.data().iter().map(|&v| v.into()).collect();
    let mut out = Image::new(cols.len(), sh - th + 1, 0f32);
    for y in 0..sh - th + 1 {
        for (i, x) in cols.clone().enumerate() {
            let (mut num, mut sum, mut sum2) = (0.0, 0.0, 0.0);
            for ty in 0..th {
                let row = &s[(y + ty) * sw + x..(y + ty) * sw + x + tw];
                for (tx, &v) in row.iter().enumerate() {
                    num += t[ty * tw + tx] * v;
                    sum += v;
                    sum2 += v * v;
                }
            }
            let denom = (sum2 - sum * sum / n).max(0.0).sqrt() * t_norm;
            // OpenCV と同じ: 分母に対して分子が大きすぎる (分散がほぼ 0) ときは 0 か ±1 にする
            let r = if num.abs() < denom {
                num / denom
            } else if num.abs() < denom * 1.125 {
                num.signum()
            } else {
                0.0
            };
            out.set(i, y, r as f32);
        }
    }
    out
}

/// 一番大きい値と、その位置 (x, y)。同じ値なら行ごとに左上から見て最初のもの (OpenCV の minMaxLoc と同じ)
pub fn max_loc(img: &Image<f32>) -> (f32, (usize, usize)) {
    let mut best = (f32::NEG_INFINITY, (0, 0));
    for y in 0..img.height() {
        for (x, &v) in img.row(y).iter().enumerate() {
            if v > best.0 {
                best = (v, (x, y));
            }
        }
    }
    best
}

/// 2 つの数の並びの相関係数 (np.corrcoef の [0, 1])
pub fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma) * (x - ma);
        sbb += (y - mb) * (y - mb);
    }
    sab / (saa * sbb).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{json_f32, json_fixture, json_image, json_u8};

    fn assert_close(got: &Image<f32>, want: &Image<f32>, tol: f32, what: &str) {
        assert_eq!(
            (got.width(), got.height()),
            (want.width(), want.height()),
            "{what} の大きさ"
        );
        let worst = got
            .data()
            .iter()
            .zip(want.data())
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(worst <= tol, "{what}: 差の最大 {worst}");
    }

    #[test]
    fn matches_opencv() {
        let f = json_fixture("cv/matching.json");
        let roi = json_image(&f["roi"], json_u8);
        let templ = json_image(&f["templ"], json_u8);
        let want = json_image(&f["result"], json_f32);
        assert_close(&match_template(&roi, &templ, None), &want, 1e-4, "8 ビット");
        // 列を絞っても、同じ列の値は変わらない
        let part = match_template(&roi, &templ, Some(3..8));
        for y in 0..part.height() {
            assert_eq!(
                part.row(y),
                &match_template(&roi, &templ, None).row(y)[3..8]
            );
        }
        let roi_f = json_image(&f["roi_f"], json_f32);
        let templ_f = json_image(&f["templ_f"], json_f32);
        assert_close(
            &match_template(&roi_f, &templ_f, None),
            &json_image(&f["result_f"], json_f32),
            1e-4,
            "浮動小数点",
        );
    }

    #[test]
    fn max_loc_takes_first() {
        let img = Image::from_vec(3, 2, vec![0.0, 2.0, 1.0, 2.0, 0.5, 2.0]);
        assert_eq!(max_loc(&img), (2.0, (1, 0)));
    }

    #[test]
    fn correlation_basics() {
        assert!((correlation(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]) - 1.0).abs() < 1e-12);
        assert!((correlation(&[1.0, 2.0, 3.0], &[3.0, 2.0, 1.0]) + 1.0).abs() < 1e-12);
    }
}
