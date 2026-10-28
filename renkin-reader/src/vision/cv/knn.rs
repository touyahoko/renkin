//! 記述子の総当たりの対応づけ (OpenCV の BFMatcher(NORM_L2).knnMatch(k=2) と同じ)
//!
//! OpenCV (Apache License 2.0) の BFMatcher(NORM_L2).knnMatch (k=2) の距離と並べ方 に合わせて、Rust で書いたもの。
//! ライセンスと著作権表示はリポジトリの THIRD_PARTY_LICENSES/OpenCV を参照

use super::sift::DESCRIPTOR_LEN;

/// 対応: 写真の特徴点 (query) と型紙の特徴点 (train) の番号と、記述子の距離
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DMatch {
    pub query: usize,
    pub train: usize,
    pub distance: f32,
}

/// 記述子を 1 バイトずつにする (SIFT の記述子は 0〜255 の整数値)
fn to_bytes(des: &[[f32; DESCRIPTOR_LEN]]) -> Vec<[u8; DESCRIPTOR_LEN]> {
    des.iter().map(|d| d.map(|v| v as u8)).collect()
}

/// 記述子の距離の 2 乗。整数で計算するので、足す順番を変えても (SIMD にしても) 値は同じ。
/// 最大でも 128 x 255^2 < 2^24 なので、float32 の 2 乗の和 (OpenCV) とも同じ値になる
#[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
fn distance_sq(a: &[u8; DESCRIPTOR_LEN], b: &[u8; DESCRIPTOR_LEN]) -> u32 {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| {
            let d = x as i32 - y as i32;
            (d * d) as u32
        })
        .sum()
}

/// wasm では自動の SIMD 化がうまくいかないので命令を直接使う。16 バイトずつ 16 ビットに広げて差をとり、
/// 隣り合う 2 つの 2 乗の和 (dot_i16x8) を 32 ビットで足していく。整数なので値は上と同じ
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
fn distance_sq(a: &[u8; DESCRIPTOR_LEN], b: &[u8; DESCRIPTOR_LEN]) -> u32 {
    use core::arch::wasm32::*;
    let mut acc = i32x4_splat(0);
    for (x, y) in a.chunks_exact(16).zip(b.chunks_exact(16)) {
        // SAFETY: どちらも 16 バイトあり、v128_load は境界の揃っていない読み込みでもよい
        let (x, y) = unsafe {
            (
                v128_load(x.as_ptr() as *const v128),
                v128_load(y.as_ptr() as *const v128),
            )
        };
        let lo = i16x8_sub(u16x8_extend_low_u8x16(x), u16x8_extend_low_u8x16(y));
        let hi = i16x8_sub(u16x8_extend_high_u8x16(x), u16x8_extend_high_u8x16(y));
        acc = i32x4_add(
            acc,
            i32x4_add(i32x4_dot_i16x8(lo, lo), i32x4_dot_i16x8(hi, hi)),
        );
    }
    (i32x4_extract_lane::<0>(acc)
        + i32x4_extract_lane::<1>(acc)
        + i32x4_extract_lane::<2>(acc)
        + i32x4_extract_lane::<3>(acc)) as u32
}

/// query の記述子ごとに、train の中で一番近いものと 2 番目に近いもの。train が 2 つ未満なら None。
/// 同じ距離なら番号の小さいほうを先にする
pub fn knn2(
    query: &[[f32; DESCRIPTOR_LEN]],
    train: &[[f32; DESCRIPTOR_LEN]],
) -> Vec<Option<(DMatch, DMatch)>> {
    let (query, train) = (to_bytes(query), to_bytes(train));
    query
        .iter()
        .enumerate()
        .map(|(qi, q)| {
            if train.len() < 2 {
                return None;
            }
            let (mut best, mut second) = ((usize::MAX, u32::MAX), (usize::MAX, u32::MAX));
            for (ti, t) in train.iter().enumerate() {
                let d = distance_sq(q, t);
                if d < best.1 {
                    second = best;
                    best = (ti, d);
                } else if d < second.1 {
                    second = (ti, d);
                }
            }
            let m = |(train, d): (usize, u32)| DMatch {
                query: qi,
                train,
                distance: (d as f32).sqrt(),
            };
            Some((m(best), m(second)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// distance_sq (wasm では SIMD 版) が、素直に 1 つずつ足したものと同じか。
    /// 差が最大 (0 と 255) になる組や、ばらばらの値の組で比べる
    #[test]
    fn distance_matches_plain_sum() {
        let mut seed = 12345u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 16) as u8
        };
        let mut pairs = vec![
            ([0u8; DESCRIPTOR_LEN], [255u8; DESCRIPTOR_LEN]),
            ([7u8; DESCRIPTOR_LEN], [7u8; DESCRIPTOR_LEN]),
        ];
        for _ in 0..100 {
            pairs.push((
                std::array::from_fn(|_| next()),
                std::array::from_fn(|_| next()),
            ));
        }
        for (a, b) in &pairs {
            let want: u32 = a
                .iter()
                .zip(b)
                .map(|(&x, &y)| (x as i32 - y as i32).pow(2) as u32)
                .sum();
            assert_eq!(distance_sq(a, b), want);
        }
    }
}
