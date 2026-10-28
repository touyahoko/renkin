//! 画像処理: 写真を画面の座標にそろえ (位置合わせ)、鑑定画面の欄を読む。
//!
//! 予定している中身:
//! - align: 位置合わせ (型紙・角の飾り・交点。MHXX の画面を知っている)
//! - read: 鑑定画面の欄の読み取り (見本、比べ直し、符号、スロット、何個目か。MHXX の画面を知っている)
//! - cv: どんな画像にも使える汎用の処理。MHXX を知らない (align と read が使い、cv は align と read を知らない)
//!   - filter (ぼかし、トップハット、膨張)、resize (拡大縮小、射影変換で写す)、matching (照合)、components (連結成分)
//!   - sift、ransac (射影変換を求める)
//!   - linalg: 小さな線形代数 (3x3 の逆行列、9x9 の固有ベクトル、SVD など)。使うのは vision の中だけ
//!
// cv の汎用の関数には、今は使っていないもの (平行移動以外の使い道など) もある
mod align;
mod assets;
#[allow(dead_code)]
pub(crate) mod cv;
#[allow(dead_code)]
mod read;

use crate::timing::{self, time};
use crate::types::Timing;
use crate::types::{Candidate, Diagnostics, Field, Fields, InputError, ReadResult, RejectReason};
use align::Aligner;
use cv::image::RgbaView;
use cv::linalg::{Mat3, inv3};
use cv::warp::transform_point;
use read::{Scored, ScreenRead, Templates};

/// 1 位の点数がこれ未満か、2 位との差がこれ未満なら、スキル名・数字は自信なし
pub const MIN_SCORE: f64 = 0.6;
pub const MIN_MARGIN: f64 = 0.05;

/// 位置合わせ後の画面の大きさ
pub const SCREEN_WIDTH: u32 = 1280;
pub const SCREEN_HEIGHT: u32 = 720;

pub struct Reader {
    aligner: Aligner,
    templates: Templates,
    init_timings: Vec<Timing>,
    /// 直前に読んだ画像の、位置合わせ後の画面 (SCREEN_WIDTH x SCREEN_HEIGHT の RGBA)
    aligned: Option<Vec<u8>>,
    /// 直前に読んだ写真の、写真の座標 → 画面の座標 (位置合わせに失敗していれば None)
    h: Option<Mat3>,
}

impl Reader {
    /// 型紙の特徴点や見本の下ごしらえなど、重い準備を一度だけやる
    pub fn new() -> Self {
        timing::take();
        let aligner = time("init.reference", Aligner::new);
        let templates = time("init.templates", Templates::load);
        Self {
            aligner,
            templates,
            init_timings: timing::take(),
            aligned: None,
            h: None,
        }
    }

    /// 準備 (new) の段階ごとの時間
    pub fn init_timings(&self) -> &[Timing] {
        &self.init_timings
    }

    /// RGBA の画素 (width x height x 4 バイト) を読む
    pub fn read(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<ReadResult, InputError> {
        let photo = RgbaView::new(rgba, width, height)?;
        self.aligned = None;
        self.h = None;
        timing::take(); // 前の読み取りの記録を消す
        let aligned = match self.aligner.align(&photo) {
            Ok(a) => a,
            Err((reason, diagnostics)) => {
                return Ok(ReadResult::Rejected {
                    reason,
                    diagnostics,
                    timings: timing::take(),
                });
            }
        };
        // 位置合わせ後の画面を RGBA にして持っておく (画面側で表示するため。透明度は 255)
        let screen: Vec<u8> = aligned
            .screen
            .data()
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect();
        let view = RgbaView::new(&screen, SCREEN_WIDTH, SCREEN_HEIGHT)
            .expect("位置合わせ後の画面は 1280x720");
        let mut result = time("read.screen", || {
            self.read_aligned(&view, aligned.diagnostics)
        });
        match &mut result {
            ReadResult::Rejected { timings, .. } | ReadResult::Read { timings, .. } => {
                *timings = timing::take()
            }
        }
        self.aligned = Some(screen);
        self.h = Some(aligned.h);
        Ok(result)
    }

    /// 直前に読んだ画像の、位置合わせ後の画面。位置合わせに失敗していれば None
    pub fn aligned_image(&self) -> Option<&[u8]> {
        self.aligned.as_deref()
    }

    /// テスト用 (劣化検証、tests/degradation.rs): 直前に読んだ写真で、画面の点 (x, y) が写真のどこに写っているか。
    /// 位置合わせに失敗していれば None
    #[doc(hidden)]
    pub fn screen_to_photo(&self, x: f32, y: f32) -> Option<[f32; 2]> {
        inv3(self.h.as_ref()?).map(|hinv| transform_point(&hinv, [x, y]))
    }

    /// 位置合わせ後の画面 (1280x720) を読む
    fn read_aligned(&self, screen: &RgbaView, diagnostics: Diagnostics) -> ReadResult {
        to_result(&read::read_screen(screen, &self.templates), diagnostics)
    }
}

/// 照合の結果を、欄の結果にする。confident は score と margin が基準を満たし、かつ extra が true のとき
fn field<T: Copy, U>(
    s: &Scored<T>,
    conv: impl Fn(T) -> U,
    min_score: f64,
    min_margin: f64,
    extra: bool,
) -> Field<U> {
    Field {
        value: conv(s.value),
        confident: s.score >= min_score && s.margin >= min_margin && extra,
        score: s.score as f32,
        margin: s.margin as f32,
        candidates: s
            .candidates
            .iter()
            .map(|&(v, score)| Candidate {
                value: conv(v),
                score: score as f32,
            })
            .collect(),
    }
}

/// 画面を読んだ結果を、TS に返す形にする。パネルのラベルが見つからなければ、どの欄も信用できないので Rejected
fn to_result(r: &ScreenRead, mut diagnostics: Diagnostics) -> ReadResult {
    diagnostics.panel_score = Some(r.panel.2 as f32);
    if r.panel.2 < read::LABEL_MIN_SCORE {
        return ReadResult::Rejected {
            reason: RejectReason::PanelNotFound,
            diagnostics,
            timings: Vec::new(),
        };
    }
    let sign_ok = |i: usize| r.signs.get(i).is_none_or(|&(p, m)| read::sign_ok(p, m));
    // スロットは 3 つの記号のうち一番悪いもので決める: 点数はよく合うほうの一致度の最小、差は丸と横線の差の最小
    let slot_score = r
        .slot_scores
        .iter()
        .map(|&(c, d)| c.max(d))
        .fold(f64::MAX, f64::min);
    let slot_margin = r
        .slot_scores
        .iter()
        .map(|&(c, d)| (c - d).abs())
        .fold(f64::MAX, f64::min);
    let slots = Scored {
        value: r.slots,
        score: slot_score,
        margin: slot_margin,
        candidates: vec![(r.slots, slot_score)],
    };
    let fields = Fields {
        skill1: field(&r.skill1, |v| v, MIN_SCORE, MIN_MARGIN, true),
        pts1: field(&r.pts1, |v| v, MIN_SCORE, MIN_MARGIN, sign_ok(0)),
        skill2: field(&r.skill2, |v| v, MIN_SCORE, MIN_MARGIN, true),
        pts2: field(&r.pts2, |v| v, MIN_SCORE, MIN_MARGIN, sign_ok(1)),
        slots: field(
            &slots,
            |v| v,
            read::SLOT_MIN_SCORE,
            read::SLOT_MIN_DIFF,
            true,
        ),
        index: field(&r.index, |v| v, read::INDEX_MIN_SCORE, f64::MIN, true),
    };
    ReadResult::Read {
        diagnostics,
        fields: Box::new(fields),
        timings: Vec::new(),
    }
}

impl Default for Reader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wrong_pixel_length() {
        let mut r = Reader::new();
        assert_eq!(
            r.read(&[0; 7], 1, 2),
            Err(InputError::PixelLength {
                expected: 8,
                actual: 7
            })
        );
    }

    #[test]
    fn tiny_image_is_not_aligned() {
        let mut r = Reader::new();
        let result = r.read(&[0; 16], 2, 2).unwrap();
        assert!(matches!(
            result,
            ReadResult::Rejected {
                reason: RejectReason::NotAligned,
                ..
            }
        ));
        assert!(r.aligned_image().is_none());
    }

    /// 変な大きさや中身の画像でも panic せず、撮り直し (か読めた結果) を返す。1 つの Reader を使い回す
    #[test]
    fn odd_images_do_not_panic() {
        let mut r = Reader::new();
        let mut seed = 7u32;
        let mut noise = |n: usize| -> Vec<u8> {
            (0..n)
                .map(|_| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    (seed >> 16) as u8
                })
                .collect()
        };
        for (w, h) in [
            (1, 1),
            (3000, 2),
            (2, 3000),
            (17, 9),
            (64, 64),
            (1280, 720),
            (4000, 300),
        ] {
            for rgba in [vec![0; w * h * 4], vec![255; w * h * 4], noise(w * h * 4)] {
                let result = r.read(&rgba, w as u32, h as u32).unwrap();
                assert!(
                    matches!(
                        result,
                        ReadResult::Rejected { .. } | ReadResult::Read { .. }
                    ),
                    "{w}x{h}"
                );
            }
        }
    }
}
