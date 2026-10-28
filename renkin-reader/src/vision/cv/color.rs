//! 色の変換
//!
//! OpenCV (Apache License 2.0) の modules/imgproc/src/color_hsv.simd.hpp の 8 ビットの RGB → HSV (整数の割り算の表) と、RGB → 灰色 (固定小数点の重み) に合わせて、Rust で書いたもの。
//! ライセンスと著作権表示はリポジトリの THIRD_PARTY_LICENSES/OpenCV を参照

use super::image::{Image, RgbSource};

/// 色チャンネル (R・G・B) の最大値。欄の読み取りで使う灰色
pub fn max_channel(src: &impl RgbSource) -> Image<u8> {
    let mut out = Image::new(src.width(), src.height(), 0u8);
    for y in 0..src.height() {
        for x in 0..src.width() {
            let [r, g, b] = src.rgb(x, y);
            out.set(x, y, r.max(g).max(b));
        }
    }
    out
}

/// 灰色 (OpenCV の COLOR_BGR2GRAY の 8 ビット版と同じ値: 重みを 2^15 倍した整数で計算する)。SIFT に使う
pub fn gray(src: &impl RgbSource) -> Image<u8> {
    let mut out = Image::new(src.width(), src.height(), 0u8);
    for y in 0..src.height() {
        for x in 0..src.width() {
            let [r, g, b] = src.rgb(x, y).map(|v| v as u32);
            out.set(
                x,
                y,
                ((b * 3735 + g * 19235 + r * 9798 + (1 << 14)) >> 15) as u8,
            );
        }
    }
    out
}

/// 色相・彩度・明度 (OpenCV の COLOR_BGR2HSV の 8 ビット版と同じ値)。色相は 0〜179、彩度と明度は 0〜255
pub struct Hsv {
    pub h: Image<u8>,
    pub s: Image<u8>,
    pub v: Image<u8>,
}

const HSV_SHIFT: i32 = 12;

/// OpenCV と同じく、割り算を整数の表 (2^12 倍して丸めた逆数) で行う
fn hsv_tables() -> ([i32; 256], [i32; 256]) {
    let mut sdiv = [0i32; 256];
    let mut hdiv = [0i32; 256];
    for i in 1..256 {
        sdiv[i] = ((255 << HSV_SHIFT) as f64 / i as f64).round() as i32;
        hdiv[i] = ((180 << HSV_SHIFT) as f64 / (6.0 * i as f64)).round() as i32;
    }
    (sdiv, hdiv)
}

pub fn hsv(src: &impl RgbSource) -> Hsv {
    let (sdiv, hdiv) = hsv_tables();
    let (w, h) = (src.width(), src.height());
    let mut out = Hsv {
        h: Image::new(w, h, 0),
        s: Image::new(w, h, 0),
        v: Image::new(w, h, 0),
    };
    for y in 0..h {
        for x in 0..w {
            let [r, g, b] = src.rgb(x, y).map(|v| v as i32);
            let v = r.max(g).max(b);
            let diff = v - r.min(g).min(b);
            // OpenCV と同じ順で場合分けする (最大が R なら G-B、G なら B-R+2diff、それ以外は R-G+4diff)
            let hue = if v == r {
                g - b
            } else if v == g {
                b - r + 2 * diff
            } else {
                r - g + 4 * diff
            };
            let s = (diff * sdiv[v as usize] + (1 << (HSV_SHIFT - 1))) >> HSV_SHIFT;
            let mut hue = (hue * hdiv[diff as usize] + (1 << (HSV_SHIFT - 1))) >> HSV_SHIFT;
            if hue < 0 {
                hue += 180;
            }
            out.h.set(x, y, hue as u8);
            out.s.set(x, y, s as u8);
            out.v.set(x, y, v as u8);
        }
    }
    out
}

/// HSV が [lo, hi] (両端を含む) に入る画素を true にする (OpenCV の inRange)
pub fn in_range(hsv: &Hsv, lo: [u8; 3], hi: [u8; 3]) -> Image<bool> {
    let (w, h) = (hsv.h.width(), hsv.h.height());
    let mut out = Image::new(w, h, false);
    for i in 0..w * h {
        let px = [hsv.h.data()[i], hsv.s.data()[i], hsv.v.data()[i]];
        out.data_mut()[i] = (0..3).all(|c| lo[c] <= px[c] && px[c] <= hi[c]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{json_fixture, json_image, json_u8};
    use crate::vision::cv::image::RgbaView;

    #[test]
    fn gray_matches_opencv() {
        let f = json_fixture("cv/align_ops.json");
        let src = crate::testutil::json_rgb(&f["src"]);
        assert_eq!(gray(&src), json_image(&f["gray"], json_u8));
    }

    #[test]
    fn matches_opencv() {
        let f = json_fixture("cv/color.json");
        let rgba: Vec<u8> = f["rgba"]["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(json_u8)
            .collect();
        let (w, h) = (
            f["rgba"]["width"].as_u64().unwrap() as u32,
            f["rgba"]["height"].as_u64().unwrap() as u32,
        );
        let view = RgbaView::new(&rgba, w, h).unwrap();
        assert_eq!(max_channel(&view), json_image(&f["max"], json_u8));
        let hsv = hsv(&view);
        assert_eq!(hsv.h, json_image(&f["h"], json_u8), "色相");
        assert_eq!(hsv.s, json_image(&f["s"], json_u8), "彩度");
        assert_eq!(hsv.v, json_image(&f["v"], json_u8), "明度");
        let yellow = in_range(&hsv, [18, 120, 150], [35, 255, 255]);
        assert_eq!(
            yellow,
            json_image(&f["yellow"], |v| v.as_u64().unwrap() == 255)
        );
    }
}
