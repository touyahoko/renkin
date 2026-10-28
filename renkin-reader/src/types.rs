//! TypeScript との境界でやり取りする型。serde で JS のオブジェクトに変える (項目名は camelCase)。

use serde::{Deserialize, Serialize};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use tsify::Tsify;

/// mhxx-rng のスキル番号
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), tsify::declare)]
pub type SkillId = u16;

/// 画像 1 枚を読んだ結果
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(Tsify),
    tsify(into_wasm_abi, missing_as_null)
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReadResult {
    /// 画像全体が信用できない。どの欄も埋めない
    Rejected {
        reason: RejectReason,
        diagnostics: Diagnostics,
        timings: Vec<Timing>,
    },
    /// 読めた。欄ごとに confident を見る
    Read {
        diagnostics: Diagnostics,
        fields: Box<Fields>,
        timings: Vec<Timing>,
    },
}

/// 段階ごとの時間 (段階の ID は src/timing.rs の一覧)。試し直しなどで同じ段階が何度も出ることがある
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), derive(Tsify))]
pub struct Timing {
    pub stage: &'static str,
    /// ミリ秒
    pub ms: f64,
}

/// 画像全体を信用できない理由。画面はこれを見て撮り直しの案内を出す
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), derive(Tsify))]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// 画面が見つからない・位置合わせに失敗した
    NotAligned,
    /// 写真の中の画面が小さすぎる
    TooSmall,
    /// パネルの位置を確かめられない
    PanelNotFound,
}

/// 判断には使わない、表示や調査用の数値。途中で失敗したときは取れた所まで
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), derive(Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    /// 写真と型紙の特徴点の対応の数
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), tsify(optional))]
    pub matches: Option<u32>,
    /// そのうち、RANSAC で正しいとされた対応の数
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), tsify(optional))]
    pub inliers: Option<u32>,
    /// 見つかった角の飾りの数
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), tsify(optional))]
    pub corners: Option<u32>,
    /// 見つかった鑑定ボックスの交点の数
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), tsify(optional))]
    pub grid: Option<u32>,
    /// 画面 → 写真の局所的な伸びの縦横比の最大 (細長さ)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), tsify(optional))]
    pub anisotropy: Option<f32>,
    /// 文字のある所で、画面 1px が写真の何 px になるか
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), tsify(optional))]
    pub resolution: Option<f32>,
    /// 左右反転した写真として読んだか
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), tsify(optional))]
    pub mirrored: Option<bool>,
    /// パネルの固定ラベルの一致度
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), tsify(optional))]
    pub panel_score: Option<f32>,
}

/// 読み取った欄
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(Tsify),
    tsify(missing_as_null)
)]
#[serde(rename_all = "camelCase")]
pub struct Fields {
    pub skill1: Field<SkillId>,
    pub pts1: Field<i8>,
    /// None (TS では null) は第 2 スキルなし
    pub skill2: Field<Option<SkillId>>,
    pub pts2: Field<Option<i8>>,
    /// 0〜3
    pub slots: Field<u8>,
    /// 鑑定ボックスの何個目か (1 から)。None (TS では null) は読めない
    pub index: Field<Option<u8>>,
}

/// 1 つの欄の読み取り結果。score と margin の測り方は欄ごとに違うので、欄どうしでは比べられない
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), derive(Tsify))]
pub struct Field<T> {
    /// 一番それらしい答え
    pub value: T,
    /// この欄の答えを信用してよいか (score と margin が基準を満たすか)
    pub confident: bool,
    /// 一番それらしい答えが、どれだけよく合ったか
    pub score: f32,
    /// 一番それらしい答えと、次の候補との差
    pub margin: f32,
    /// value を含む上位いくつかの候補 (点数の高い順)
    pub candidates: Vec<Candidate<T>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), derive(Tsify))]
pub struct Candidate<T> {
    pub value: T,
    pub score: f32,
}

/// フレームの探索に渡すお守り (画面でユーザーが確かめた値)
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(
    all(target_arch = "wasm32", target_os = "unknown"),
    derive(Tsify),
    tsify(from_wasm_abi, missing_as_null)
)]
#[serde(rename_all = "camelCase")]
pub struct Charm {
    pub skill1: SkillId,
    pub pts1: i8,
    pub skill2: Option<SkillId>,
    pub pts2: Option<i8>,
    pub slots: u8,
}

/// マカフシギ錬金で出るスキルと、そのポイントの範囲 (画面の選択肢に使う)。範囲は錬金の表 (風化した護石と
/// 古びた護石) を合わせたもので、そのスキルになれない欄は null
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), derive(Tsify))]
pub struct SkillInfo {
    pub id: SkillId,
    /// 鑑定画面の表記
    pub name: &'static str,
    /// 第 1 スキルとしてのポイントの範囲
    pub skill1: Option<PointRange>,
    /// 第 2 スキルとしてのポイントの範囲 (錬金ではマイナスは出ないので 1 から)
    pub skill2: Option<PointRange>,
}

/// ポイントの範囲 (両端を含む)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), derive(Tsify))]
pub struct PointRange {
    pub min: i8,
    pub max: i8,
}

impl PointRange {
    pub fn contains(&self, p: i8) -> bool {
        (self.min..=self.max).contains(&p)
    }

    /// 2 つの範囲を合わせた範囲
    fn union(self, o: Self) -> Self {
        Self {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }
}

/// 範囲を合わせる (片方が無ければもう片方)
pub(crate) fn union_range(a: Option<PointRange>, b: Option<PointRange>) -> Option<PointRange> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    }
}

/// フレームの探索の結果の 1 件: そのフレームで錬金すると、探した護石が何個目に出るか
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), derive(Tsify))]
pub struct FrameCandidate {
    pub frame: u32,
    /// 何個目か (1 から、小さい順)。同じ中身の護石が 1 回の錬金で 2 か所に出ることもあるので一覧
    pub indexes: Vec<u8>,
}

/// 入力の誤り (呼び出し側のバグ)。読み取りの失敗は ReadResult::Rejected で返し、これは使わない
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    /// rgba の長さが width x height x 4 でない
    PixelLength { expected: usize, actual: usize },
    /// 探索の範囲が start > end
    FrameRange { start: u32, end: u32 },
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PixelLength { expected, actual } => {
                write!(
                    f,
                    "rgba の長さが width x height x 4 ({expected}) と違う ({actual})"
                )
            }
            Self::FrameRange { start, end } => {
                write!(f, "探索の範囲が逆 (start {start} > end {end})")
            }
        }
    }
}

impl std::error::Error for InputError {}
