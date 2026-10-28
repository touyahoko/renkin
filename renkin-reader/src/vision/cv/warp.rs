//! 射影変換で画像と点を写す。
//!
//! OpenCV (Apache License 2.0) の warpPerspective (modules/imgproc/src/imgwarp.cpp、warp_kernels.simd.hpp の
//! warpPerspectiveLinearInvoker の SIMD 版: float32 の FMA で座標と補間を計算する) と perspectiveTransform に合わせて、
//! Rust で書いたもの。ライセンスと著作権表示はリポジトリの THIRD_PARTY_LICENSES/OpenCV を参照

use std::ops::Range;

use super::image::{Image, Rgb, RgbSource};
use super::linalg::{Mat3, inv3};

/// 点 (x, y) を h で写す (cv2.perspectiveTransform の float32 版: float64 で計算して float32 に落とす)
pub fn transform_point(h: &Mat3, p: [f32; 2]) -> [f32; 2] {
    let (x, y) = (p[0] as f64, p[1] as f64);
    let w = x * h[2][0] + y * h[2][1] + h[2][2];
    if w.abs() <= f32::EPSILON as f64 {
        return [0.0, 0.0];
    }
    let w = 1.0 / w;
    [
        ((x * h[0][0] + y * h[0][1] + h[0][2]) * w) as f32,
        ((x * h[1][0] + y * h[1][1] + h[1][2]) * w) as f32,
    ]
}

/// src を h (src の座標 → 出力の座標) で写した、width x height の画像 (線形補間、src の外は 0)。
/// cv2.warpPerspective(src, h, (width, height), flags=INTER_LINEAR) と同じ。h が逆にできなければ全部 0
pub fn warp_perspective(src: &impl RgbSource, h: &Mat3, width: usize, height: usize) -> Image<Rgb> {
    warp_perspective_rect(src, h, 0..width, 0..height)
}

/// warp_perspective の出力のうち、xs x ys の範囲だけ。画素ごとに出力の座標から独立に計算するので、
/// 全体を写してから切り出したものと同じになる
pub fn warp_perspective_rect(
    src: &impl RgbSource,
    h: &Mat3,
    xs: Range<usize>,
    ys: Range<usize>,
) -> Image<Rgb> {
    let mut out = Image::new(xs.len(), ys.len(), [0u8; 3]);
    let Some(inv) = inv3(h) else { return out };
    let m: Vec<f32> = inv.iter().flatten().map(|&v| v as f32).collect();
    let (cols, rows) = (src.width() as i64, src.height() as i64);
    let fetch = |x: i64, y: i64| -> [f32; 3] {
        if (0..cols).contains(&x) && (0..rows).contains(&y) {
            src.rgb(x as usize, y as usize).map(f32::from)
        } else {
            [0.0; 3]
        }
    };
    for (oy, y) in ys.enumerate() {
        let yf = y as f32;
        let (mx, my, mw) = (yf * m[1] + m[2], yf * m[4] + m[5], yf * m[7] + m[8]);
        for (ox, x) in xs.clone().enumerate() {
            let xf = x as f32;
            let w = m[6].mul_add(xf, mw);
            let sx = m[0].mul_add(xf, mx) / w;
            let sy = m[3].mul_add(xf, my) / w;
            let (ix, iy) = (sx.floor() as i64, sy.floor() as i64);
            // 4 画素とも src の外なら 0 のまま
            if ix + 1 < 0 || ix + 1 > cols || iy + 1 < 0 || iy + 1 > rows {
                continue;
            }
            let (ax, ay) = (sx - ix as f32, sy - iy as f32);
            let (p00, p01, p10, p11) = (
                fetch(ix, iy),
                fetch(ix + 1, iy),
                fetch(ix, iy + 1),
                fetch(ix + 1, iy + 1),
            );
            let px = [0, 1, 2].map(|c| {
                let v0 = ax.mul_add(p01[c] - p00[c], p00[c]);
                let v1 = ax.mul_add(p11[c] - p10[c], p10[c]);
                ay.mul_add(v1 - v0, v0).round_ties_even().clamp(0.0, 255.0) as u8
            });
            out.set(ox, oy, px);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{json_fixture, json_rgb};

    #[test]
    fn warp_matches_opencv() {
        let f = json_fixture("cv/align_ops.json");
        let src = json_rgb(&f["src"]);
        let h: Mat3 =
            std::array::from_fn(|r| std::array::from_fn(|c| f["h"][r][c].as_f64().unwrap()));
        let want = json_rgb(&f["warp"]);
        let got = warp_perspective(&src, &h, want.width(), want.height());
        let diff: Vec<i32> = got
            .data()
            .iter()
            .zip(want.data())
            .flat_map(|(a, b)| (0..3).map(move |c| a[c] as i32 - b[c] as i32))
            .filter(|&d| d != 0)
            .collect();
        println!(
            "warpPerspective: 違う値 {} 個 / {}、最大 {}",
            diff.len(),
            want.data().len() * 3,
            diff.iter().map(|d| d.abs()).max().unwrap_or(0)
        );
        assert!(diff.is_empty());
    }
}
