//! 画像の型

use crate::types::InputError;

/// 1 チャンネルの画像 (灰色、色チャンネルの最大値、照合の結果など)。画素は行ごとに左から右へ並ぶ
#[derive(Debug, Clone, PartialEq)]
pub struct Image<T> {
    width: usize,
    height: usize,
    data: Vec<T>,
}

impl<T: Copy> Image<T> {
    /// 全部の画素が fill の画像
    pub fn new(width: usize, height: usize, fill: T) -> Self {
        Self {
            width,
            height,
            data: vec![fill; width * height],
        }
    }

    /// 画素の並びから作る. data の長さは width x height
    pub fn from_vec(width: usize, height: usize, data: Vec<T>) -> Self {
        assert_eq!(data.len(), width * height, "画素の数が幅 x 高さと違う");
        Self {
            width,
            height,
            data,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn data(&self) -> &[T] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [T] {
        &mut self.data
    }

    pub fn get(&self, x: usize, y: usize) -> T {
        self.data[y * self.width + x]
    }

    pub fn set(&mut self, x: usize, y: usize, value: T) {
        self.data[y * self.width + x] = value;
    }

    /// y 行目の画素
    pub fn row(&self, y: usize) -> &[T] {
        &self.data[y * self.width..(y + 1) * self.width]
    }

    /// 左上 (x, y)、幅 w、高さ h の部分を切り出す (はみ出してはいけない)
    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Self {
        assert!(
            x + w <= self.width && y + h <= self.height,
            "切り出す範囲が画像からはみ出す"
        );
        let data = (y..y + h)
            .flat_map(|yy| self.row(yy)[x..x + w].iter().copied())
            .collect();
        Self {
            width: w,
            height: h,
            data,
        }
    }

    /// 画素ごとに f を通した画像
    pub fn map<U: Copy>(&self, f: impl Fn(T) -> U) -> Image<U> {
        Image {
            width: self.width,
            height: self.height,
            data: self.data.iter().map(|&v| f(v)).collect(),
        }
    }
}

/// JS から受け取った RGBA の画素 (1 画素 4 バイト、R・G・B・A の順) を、コピーせずに見る。A は使わない
#[derive(Debug, Clone, Copy)]
pub struct RgbaView<'a> {
    width: usize,
    height: usize,
    data: &'a [u8],
}

impl<'a> RgbaView<'a> {
    /// data の長さが width x height x 4 でなければ誤り
    pub fn new(data: &'a [u8], width: u32, height: u32) -> Result<Self, InputError> {
        let (width, height) = (width as usize, height as usize);
        let expected = width * height * 4;
        if data.len() != expected {
            return Err(InputError::PixelLength {
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// (x, y) の [R, G, B, A]
    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.width + x) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }
}

/// RGB の画素
pub type Rgb = [u8; 3];

/// RGB の画素を読める画像 (JS から受け取った RGBA、縮小した RGB の画像、左右反転して読む写真)
pub trait RgbSource {
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn rgb(&self, x: usize, y: usize) -> Rgb;
    /// y 行目を out にまとめて読む (out の長さは幅)
    fn read_row(&self, y: usize, out: &mut [Rgb]) {
        for (x, p) in out.iter_mut().enumerate() {
            *p = self.rgb(x, y);
        }
    }
}

impl RgbSource for RgbaView<'_> {
    fn width(&self) -> usize {
        self.width
    }
    fn height(&self) -> usize {
        self.height
    }
    fn rgb(&self, x: usize, y: usize) -> Rgb {
        let i = (y * self.width + x) * 4;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }
    fn read_row(&self, y: usize, out: &mut [Rgb]) {
        let row = &self.data[y * self.width * 4..(y + 1) * self.width * 4];
        for (p, q) in out.iter_mut().zip(row.chunks_exact(4)) {
            *p = [q[0], q[1], q[2]];
        }
    }
}

impl RgbSource for Image<Rgb> {
    fn width(&self) -> usize {
        self.width
    }
    fn height(&self) -> usize {
        self.height
    }
    fn rgb(&self, x: usize, y: usize) -> Rgb {
        self.get(x, y)
    }
    fn read_row(&self, y: usize, out: &mut [Rgb]) {
        out.copy_from_slice(self.row(y));
    }
}

/// 左右反転して読む (コピーしない)。インカメラなどで反転して保存された写真の試し直しに使う
pub struct Flipped<'a, S: RgbSource>(pub &'a S);

impl<S: RgbSource> RgbSource for Flipped<'_, S> {
    fn width(&self) -> usize {
        self.0.width()
    }
    fn height(&self) -> usize {
        self.0.height()
    }
    fn rgb(&self, x: usize, y: usize) -> Rgb {
        self.0.rgb(self.0.width() - 1 - x, y)
    }
    fn read_row(&self, y: usize, out: &mut [Rgb]) {
        self.0.read_row(y, out);
        out.reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, load_png_rgba};

    #[test]
    fn rgba_view_checks_length() {
        assert!(RgbaView::new(&[0; 8], 1, 2).is_ok());
        assert_eq!(
            RgbaView::new(&[0; 7], 1, 2).unwrap_err(),
            InputError::PixelLength {
                expected: 8,
                actual: 7
            }
        );
    }

    #[test]
    fn image_indexing() {
        let mut img = Image::from_vec(3, 2, vec![0u8, 1, 2, 3, 4, 5]);
        assert_eq!(img.get(2, 1), 5);
        assert_eq!(img.row(1), &[3, 4, 5]);
        img.set(0, 1, 9);
        assert_eq!(img.get(0, 1), 9);
    }

    /// PNG (RGBA・RGB・灰色) を読んだ画素が、OpenCV で読んで RGBA に並べ直したものと同じか
    #[test]
    fn png_loads_like_opencv() {
        let expected: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fixture("load/expected.json")).unwrap())
                .unwrap();
        for name in ["rgba.png", "rgb.png", "gray.png"] {
            let e = &expected[name];
            let (data, w, h) = load_png_rgba(&fixture(&format!("load/{name}")));
            assert_eq!(
                (w as u64, h as u64),
                (e["width"].as_u64().unwrap(), e["height"].as_u64().unwrap()),
                "{name}"
            );
            let want: Vec<u8> = e["rgba"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect();
            assert_eq!(data, want, "{name}");
            let view = RgbaView::new(&data, w, h).unwrap();
            assert_eq!(
                view.pixel(w as usize - 1, 0),
                want[(w as usize - 1) * 4..w as usize * 4],
                "{name}"
            );
        }
    }
}
