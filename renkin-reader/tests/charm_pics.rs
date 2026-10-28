//! charm-pics の全部の画像 (tests と synthetic。実際の写真) を、写真から最後まで読み、answers.csv の期待どおりかを
//! 確かめる (判定は tests/support/e2e.rs)。charm-pics は公開しないリポジトリなので、このテストはそれがあるときだけ動く。
//!
//! 重いので `cargo test --release --test charm_pics -- --ignored --nocapture` で流す。charm-pics の場所は環境変数
//! CHARM_PICS (無ければ、このリポジトリの隣の charm-pics)。無ければ何もせずに通る。HEIC は Rust で開けないので、
//! tools/export_golden.py が tests/golden/photos/ に書き出した JPEG を使う。
//!
//! wasm の中でも流せる (スレッドは使えないので 1 本で読む。数分かかる):
//! `cargo test --release --target wasm32-wasip1 --test charm_pics -- --ignored --nocapture`
//!
//! 調べるとき用の環境変数は tests/support/e2e.rs のほかに、RENKIN_DECODED (OpenCV で開いた画素
//! tests/golden/photos/<名前>.png があればそれを使う。開き方の違いを調べるとき用)

#[path = "support/e2e.rs"]
mod e2e;

use std::path::{Path, PathBuf};

/// 期待と違うことがわかっていて、許しているもの (名前, 理由)
const KNOWN: &[(&str, &str)] = &[(
    "synthetic/photos/001/PXL_20261018_134214292.jpg",
    "JPEG の開き方の違い (Rust の image と OpenCV の libjpeg-turbo) でパネルが見つからない。OpenCV で開いた画素なら読める",
)];

fn charm_pics() -> PathBuf {
    std::env::var("CHARM_PICS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../charm-pics"))
}

/// HEIC と RENKIN_DECODED のときは、tests/golden/photos/ に書き出したものを開く
fn open(a: &e2e::Answer) -> (Vec<u8>, u32, u32) {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/photos");
    let decoded = golden.join(a.name.replace('/', "__") + ".png");
    let ext = a.path.extension().unwrap().to_string_lossy().to_lowercase();
    let path = if std::env::var("RENKIN_DECODED").is_ok() && decoded.is_file() {
        decoded
    } else if ext == "heic" || ext == "heif" {
        let jpg = golden.join(a.name.replace('/', "__") + ".jpg");
        assert!(
            jpg.is_file(),
            "{} が無い (tools/export_golden.py で作る)",
            jpg.display()
        );
        jpg
    } else {
        a.path.clone()
    };
    e2e::load(&path)
}

#[test]
#[ignore]
fn all_images_as_expected() {
    let root = charm_pics();
    if !root.join("tests").is_dir() {
        println!("charm-pics が無いので飛ばす ({})", root.display());
        return;
    }
    let (all, results) = e2e::run(e2e::answers(&root, &["tests", "synthetic"]), open);
    let unexpected = e2e::summarize(&all, &results, KNOWN);
    assert!(unexpected.is_empty());
}
