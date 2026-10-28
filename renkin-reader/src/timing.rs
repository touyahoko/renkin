//! 段階ごとの時間の計測。読み取りの結果 (ReadResult の timings) と、準備の内訳 (Reader::init_timings) で返す。
//!
//! 測りたい処理を `time("段階の ID", || 処理)` で包むだけでよい。記録はスレッドごとで、Reader::read が読み取りの
//! 始めに消し、終わりに取り出して結果に付ける。段階の ID (TS 側が表示や集計に使う):
//!
//! | ID | 中身 |
//! |---|---|
//! | init.reference | 準備: 埋め込んだ位置合わせの型紙の特徴点を取り出す |
//! | init.templates | 準備: 見本の下ごしらえ |
//! | align.resize | 写真を長辺 2000px にそろえる |
//! | align.gray | 灰色にする |
//! | sift.initial | SIFT: 最初の画像 (2 倍に拡大してぼかす) |
//! | sift.pyramid | SIFT: ガウシアンのピラミッド (段ごとに記録) |
//! | sift.dog | SIFT: DoG (ぼかした画像どうしの差。段ごとに記録) |
//! | sift.extrema | SIFT: 極値の探索と向き (段ごとに記録) |
//! | sift.descriptors | SIFT: 記述子 |
//! | align.match | 写真と型紙の特徴点の総当たりの対応づけ |
//! | align.ransac | RANSAC で射影変換を求める |
//! | align.corners | 角の飾りで当て直す |
//! | align.grid | 鑑定ボックスの交点で当て直す |
//! | align.warp | 写真を画面 (1280x720) に写す |
//! | read.screen | 画面の欄を読む |
//!
//! 位置合わせを試し直したとき (1 対 1、左右反転) は、同じ段階が試した回数だけ出てくる。
//! 段階は入れ子になることがある (位置合わせの中で sift.* も記録されるなど) ので、全部を足すと全体より大きくなる

use std::cell::RefCell;

use crate::types::Timing;

thread_local! {
    static RECORDS: RefCell<Vec<Timing>> = const { RefCell::new(Vec::new()) };
}

/// 今の時刻 (ミリ秒)。ブラウザ向けの wasm では Rust の時計が使えないので、JS の performance.now() を使う
/// (wasm32-wasip1 では WASI の時計で Instant が使える)
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn now_ms() -> f64 {
    #[wasm_bindgen::prelude::wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = performance, js_name = now)]
        fn performance_now() -> f64;
    }
    performance_now()
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn now_ms() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

/// f を実行し、かかった時間を stage の名前で記録する
pub fn time<T>(stage: &'static str, f: impl FnOnce() -> T) -> T {
    let t = now_ms();
    let out = f();
    let ms = now_ms() - t;
    RECORDS.with(|r| r.borrow_mut().push(Timing { stage, ms }));
    out
}

/// 記録した時間を、記録した順に取り出して消す
pub fn take() -> Vec<Timing> {
    RECORDS.with(|r| std::mem::take(&mut *r.borrow_mut()))
}
