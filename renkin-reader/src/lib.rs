//! MHXX の錬金結果の鑑定画面を写真から読み、そのお守りが出る錬金のフレームを探す。
//!
//! 本体は wasm に依存しない普通の Rust で、ブラウザ用の入口 (wasm.rs) はブラウザ向け (wasm32-unknown-unknown) に作るときだけ入る
//! (wasm32-wasip1 は wasm の中でテストを流す用で、入口は入らない)。

mod rng;
mod skills;
#[cfg(test)]
mod testutil;
mod timing;
mod types;
mod vision;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod wasm;

pub use rng::{search_frames, skill_info};
pub use types::*;
pub use vision::{Reader, SCREEN_HEIGHT, SCREEN_WIDTH};

/// マカフシギ錬金で出るスキルの (番号, 表示名)。番号の小さい順
pub fn skills() -> &'static [(SkillId, &'static str)] {
    &skills::SKILLS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_are_unique_and_sorted() {
        let s = skills();
        assert_eq!(s.len(), 134);
        assert!(s.windows(2).all(|w| w[0].0 < w[1].0));
    }
}
