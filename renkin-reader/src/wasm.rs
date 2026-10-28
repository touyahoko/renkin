//! ブラウザ (TypeScript) から呼ぶ入口。本体を呼んで、結果を JS のオブジェクトに変えるだけ

use wasm_bindgen::prelude::*;

use crate::types::{Charm, ReadResult};

#[wasm_bindgen(js_name = Reader)]
pub struct WasmReader(crate::Reader);

#[wasm_bindgen(js_class = Reader)]
impl WasmReader {
    /// 重い準備を一度だけやる
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmReader {
        WasmReader(crate::Reader::new())
    }

    /// RGBA の画素 (ImageData.data など。width x height x 4 バイト) を読む
    pub fn read(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<ReadResult, JsError> {
        Ok(self.0.read(rgba, width, height)?)
    }

    /// 準備 (new Reader) の段階ごとの時間
    #[wasm_bindgen(js_name = initTimings, unchecked_return_type = "Timing[]")]
    pub fn init_timings(&self) -> Result<JsValue, JsError> {
        Ok(serde_wasm_bindgen::to_value(self.0.init_timings())?)
    }

    /// 直前に読んだ画像の、位置合わせ後の画面 (1280 x 720 の RGBA)。位置合わせに失敗していれば undefined
    #[wasm_bindgen(js_name = alignedImage)]
    pub fn aligned_image(&self) -> Option<Vec<u8>> {
        self.0.aligned_image().map(<[u8]>::to_vec)
    }
}

/// マカフシギ錬金で出るスキルと、第 1・第 2 スキルとしてのポイントの範囲 (番号の小さい順)
#[wasm_bindgen(unchecked_return_type = "SkillInfo[]")]
pub fn skills() -> Result<JsValue, JsError> {
    Ok(serde_wasm_bindgen::to_value(&crate::skill_info())?)
}

/// [start, end) のフレームのうち、charm が (index があればその何個目に) 出るものを、何個目に出るかと一緒に返す。
/// index が 5 以上 (鑑定ボックスに何回分もの錬金の護石が並んでいるとき) は、何個目の指定なしとして探す
#[wasm_bindgen(js_name = searchFrames, unchecked_return_type = "FrameCandidate[]")]
pub fn search_frames(
    charm: Charm,
    index: Option<u8>,
    start: u32,
    end: u32,
) -> Result<JsValue, JsError> {
    let found = crate::search_frames(&charm, index, start, end)?;
    Ok(serde_wasm_bindgen::to_value(&found)?)
}
