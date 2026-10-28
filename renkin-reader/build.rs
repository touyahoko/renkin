//! assets/ の見本 (8 ビットの灰色の PNG) を画素に展開して埋め込む。
//!
//! 画素は 1 つのバイナリ (OUT_DIR/assets.bin) にまとめ、本体から include_bytes! で埋め込む。目次 (名前、幅、高さ、
//! バイナリの中の位置) は Rust のソース (OUT_DIR/assets_index.rs) に書き出して include! で取り込む。
//! 実行時に PNG を展開しないので、wasm に PNG の展開処理は入らない。見本の下ごしらえ (トップハットなど) は
//! 実行時に本体の関数でやる (ここではしない)。
//! 位置合わせの型紙 (reference.png) は画素ではなく、求めた特徴量 (assets/reference_features.bin) を本体が埋め込む

use std::{env, fs, path::Path};

fn load(path: &Path) -> (u32, u32, Vec<u8>) {
    let file = fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(
        info.color_type,
        png::ColorType::Grayscale,
        "{}: 灰色の PNG ではない",
        path.display()
    );
    assert_eq!(
        info.bit_depth,
        png::BitDepth::Eight,
        "{}: 8 ビットではない",
        path.display()
    );
    (info.width, info.height, buf[..info.buffer_size()].to_vec())
}

/// path の見本を blob に足し、目次の 1 件 (Rust のソース) を返す
fn entry(path: &Path, blob: &mut Vec<u8>) -> String {
    let (w, h, px) = load(path);
    let name = path.file_stem().unwrap().to_str().unwrap();
    let offset = blob.len();
    blob.extend_from_slice(&px);
    format!("Asset {{ name: {name:?}, width: {w}, height: {h}, offset: {offset} }}")
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut blob = Vec::new();
    let mut index = String::from("// build.rs が生成。手で編集しない\n");
    for sub in ["skills", "digits", "signs", "slots"] {
        let mut files: Vec<_> = fs::read_dir(dir.join(sub))
            .unwrap_or_else(|e| panic!("assets/{sub}: {e}"))
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "png"))
            .collect();
        files.sort();
        let entries: Vec<String> = files.iter().map(|p| entry(p, &mut blob)).collect();
        index.push_str(&format!(
            "pub static {}: &[Asset] = &[{}];\n",
            sub.to_uppercase(),
            entries.join(", ")
        ));
    }
    for (name, file) in [("LABEL", "label.png"), ("GRID_DOT", "grid_dot.png")] {
        index.push_str(&format!(
            "pub static {name}: Asset = {};\n",
            entry(&dir.join(file), &mut blob)
        ));
    }

    let out = Path::new(&env::var("OUT_DIR").unwrap()).to_path_buf();
    fs::write(out.join("assets.bin"), blob).unwrap();
    fs::write(out.join("assets_index.rs"), index).unwrap();
}
