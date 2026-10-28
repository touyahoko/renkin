//! どんな画像にも使える汎用の処理。MHXX の画面のことは知らない (align と read が使う)。
//!
//! 結果は OpenCV (Python 版が使っているもの) と同じになるように作り、tests/fixtures/cv/ の
//! OpenCV の出力と比べてテストする (tools/make_fixtures.py が作る)

pub mod color;
pub mod components;
pub mod filter;
pub mod image;
pub mod knn;
pub mod linalg;
pub mod matching;
pub mod ransac;
pub mod resize;
pub mod sift;
pub mod warp;
