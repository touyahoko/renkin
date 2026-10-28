//! テストだけで使う道具 (比べるデータの場所、PNG の読み込み)

use std::path::{Path, PathBuf};

/// tests/fixtures/ の下のファイル
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// PNG を読み、(RGBA の画素, 幅, 高さ) を返す。RGB や灰色の PNG も RGBA に揃える (透明度が無ければ 255)
pub fn load_png_rgba(path: &Path) -> (Vec<u8>, u32, u32) {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    // パレットや 16 ビットを 8 ビットの画素に展開する
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    let px = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        png::ColorType::GrayscaleAlpha => px
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => unreachable!("normalize_to_color8 で展開される"),
    };
    (rgba, info.width, info.height)
}

/// tests/fixtures/ の下の JSON を読む
pub fn json_fixture(name: &str) -> serde_json::Value {
    let path = fixture(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

/// {"width", "height", "data"} を画像にする (make_fixtures.py の _img の形)
pub fn json_image<T: Copy>(
    v: &serde_json::Value,
    conv: impl Fn(&serde_json::Value) -> T,
) -> crate::vision::cv::image::Image<T> {
    let w = v["width"].as_u64().unwrap() as usize;
    let h = v["height"].as_u64().unwrap() as usize;
    let data = v["data"].as_array().unwrap().iter().map(conv).collect();
    crate::vision::cv::image::Image::from_vec(w, h, data)
}

pub fn json_u8(v: &serde_json::Value) -> u8 {
    v.as_u64().unwrap() as u8
}

pub fn json_f32(v: &serde_json::Value) -> f32 {
    v.as_f64().unwrap() as f32
}

/// {"width", "height", "data"} (RGB の並び) を RGB の画像にする (make_fixtures.py の _rgb の形)
pub fn json_rgb(v: &serde_json::Value) -> crate::vision::cv::image::Image<[u8; 3]> {
    let w = v["width"].as_u64().unwrap() as usize;
    let h = v["height"].as_u64().unwrap() as usize;
    let px: Vec<u8> = v["data"].as_array().unwrap().iter().map(json_u8).collect();
    crate::vision::cv::image::Image::from_vec(
        w,
        h,
        px.chunks(3).map(|p| [p[0], p[1], p[2]]).collect(),
    )
}
