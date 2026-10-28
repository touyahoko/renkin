//! 埋め込んだ見本 (build.rs が assets/ の PNG を画素に展開したもの)

use super::cv::image::Image;

/// 見本 1 枚の目次: 名前 (PNG のファイル名から拡張子を除いたもの)、大きさ、埋め込んだ画素の中の位置
#[derive(Debug, Clone, Copy)]
pub struct Asset {
    pub name: &'static str,
    pub width: usize,
    pub height: usize,
    pub offset: usize,
}

static PIXELS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/assets.bin"));

include!(concat!(env!("OUT_DIR"), "/assets_index.rs"));

impl Asset {
    /// 見本の画像 (8 ビットの灰色)
    pub fn image(&self) -> Image<u8> {
        Image::from_vec(
            self.width,
            self.height,
            PIXELS[self.offset..self.offset + self.width * self.height].to_vec(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, load_png_rgba};

    /// 埋め込んだ画素が、元の PNG と同じか
    #[test]
    fn embedded_pixels_match_png() {
        let groups: [(&str, &[Asset]); 4] = [
            ("skills", SKILLS),
            ("digits", DIGITS),
            ("signs", SIGNS),
            ("slots", SLOTS),
        ];
        assert_eq!(SKILLS.len(), 134);
        assert_eq!(DIGITS.len(), 13);
        for (sub, assets) in groups {
            for a in assets {
                let path = fixture("").join(format!("../../assets/{sub}/{}.png", a.name));
                let (rgba, w, h) = load_png_rgba(&path);
                let gray: Vec<u8> = rgba.chunks(4).map(|p| p[0]).collect();
                assert_eq!(
                    a.image(),
                    Image::from_vec(w as usize, h as usize, gray),
                    "{sub}/{}",
                    a.name
                );
            }
        }
        let (_, w, h) = load_png_rgba(&fixture("../../assets/label.png"));
        assert_eq!((LABEL.width, LABEL.height), (w as usize, h as usize));
    }
}
