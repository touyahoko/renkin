//! 矩形の収縮・膨張とトップハット (画像の外は無視する。OpenCV の既定の境界の扱いと同じ)、ガウシアンのぼかし
//!
//! OpenCV (Apache License 2.0) の modules/imgproc/src/morph.dispatch.cpp の矩形の収縮・膨張 (既定の境界の扱い) と、
//! getGaussianKernel・分離可能な畳み込み (BORDER_REFLECT_101) に合わせて、Rust で書いたもの。
//! ライセンスと著作権表示はリポジトリの THIRD_PARTY_LICENSES/OpenCV を参照

use super::image::Image;

/// 各画素の、k x k の窓 (中心がその画素) の中で op を畳み込んだ値。縦横に分けて計算する
fn rect_filter<T: Copy>(src: &Image<T>, k: usize, op: impl Fn(T, T) -> T + Copy) -> Image<T> {
    let (w, h) = (src.width(), src.height());
    let r = k / 2;
    let mut tmp = src.clone();
    for y in 0..h {
        let row = src.row(y);
        for x in 0..w {
            let v = row[x.saturating_sub(r)..(x + r + 1).min(w)]
                .iter()
                .copied()
                .reduce(op)
                .unwrap();
            tmp.set(x, y, v);
        }
    }
    let mut out = tmp.clone();
    for y in 0..h {
        for x in 0..w {
            let v = (y.saturating_sub(r)..(y + r + 1).min(h))
                .map(|yy| tmp.get(x, yy))
                .reduce(op)
                .unwrap();
            out.set(x, y, v);
        }
    }
    out
}

/// 収縮 (k x k の窓の最小値)。k は奇数
pub fn erode<T: Copy + PartialOrd>(src: &Image<T>, k: usize) -> Image<T> {
    rect_filter(src, k, |a, b| if b < a { b } else { a })
}

/// 膨張 (k x k の窓の最大値)。k は奇数
pub fn dilate<T: Copy + PartialOrd>(src: &Image<T>, k: usize) -> Image<T> {
    rect_filter(src, k, |a, b| if b > a { b } else { a })
}

/// トップハット: 元の画像から、オープニング (収縮してから膨張) を引く。窓より細い明るい部分 (文字) だけが残る
pub fn tophat(src: &Image<u8>, k: usize) -> Image<u8> {
    let open = dilate(&erode(src, k), k);
    let data = src
        .data()
        .iter()
        .zip(open.data())
        .map(|(&a, &b)| a.saturating_sub(b))
        .collect();
    Image::from_vec(src.width(), src.height(), data)
}

/// ガウシアンの重み (ksize 個、合計 1)。denom は exp(-x^2 / denom) の denom (= 2 sigma^2)。
/// float64 で計算して float32 に落とす (OpenCV の getGaussianKernel、NumPy 版の SIFT の gaussian_kernel と同じ)
pub fn gaussian_kernel(ksize: usize, denom: f64) -> Vec<f32> {
    let half = (ksize as f64 - 1.0) * 0.5;
    let k: Vec<f64> = (0..ksize)
        .map(|i| (-(i as f64 - half).powi(2) / denom).exp())
        .collect();
    let sum: f64 = k.iter().sum();
    k.iter().map(|v| (v / sum) as f32).collect()
}

/// 対称の畳み込みで、レジスタで足し込む塊の大きさ (画素)
const CH: usize = 32;

/// 横方向の 1 行: 両端を折り返した行 pad (後ろに CH 画素以上の余白) から、out の各画素に対称の重み k を掛けて足す。
/// 出力を CH 画素の塊ごとに、固定長の配列でレジスタに足し込んでから書く
#[inline(never)]
fn horizontal_row(pad: &[f32], k: &[f32], out: &mut [f32]) {
    let r = k.len() / 2;
    for (n, chunk) in out.chunks_mut(CH).enumerate() {
        let x0 = n * CH;
        let center: &[f32; CH] = pad[x0 + r..x0 + r + CH].try_into().unwrap();
        let mut acc = [0f32; CH];
        for j in 0..CH {
            acc[j] = k[r] * center[j];
        }
        for i in 1..=r {
            let a: &[f32; CH] = pad[x0 + r - i..x0 + r - i + CH].try_into().unwrap();
            let b: &[f32; CH] = pad[x0 + r + i..x0 + r + i + CH].try_into().unwrap();
            let kk = k[r + i];
            for j in 0..CH {
                acc[j] += kk * (a[j] + b[j]);
            }
        }
        let m = chunk.len();
        chunk.copy_from_slice(&acc[..m]);
    }
}

/// 横、縦の順に、左右対称な重み k で畳み込む。真ん中の重みを掛けた値に、中心から等しい距離の 2 画素を足してから
/// 重みを掛けたものを、近いほうから足していく (OpenCV の対称な畳み込みと同じ考え。掛け算がほぼ半分で済む)。
/// 画像の外は端で折り返す (BORDER_REFLECT_101)。
///
/// 横方向は、出力を CH 画素の塊ごとに、全部の重みを足し込んでから 1 回だけ書く。塊は固定長の配列として扱うので、
/// 範囲チェックが消えてコンパイラが SIMD にできる (スライスのままだと SIMD にならず、4 倍以上遅かった)。
/// 縦方向は、重みごとに行全体へ足す (行をまるごと順に読めるので、塊ごとにするより速い)。
/// 途中の結果 (横方向だけかけた 2r+1 行) は scratch を使い回す (新しく確保すると、最初の書き込みが遅いため)
pub fn sep_filter_symmetric_reflect101(
    src: &Image<f32>,
    k: &[f32],
    scratch: &mut Vec<f32>,
) -> Image<f32> {
    let r = k.len() / 2;
    let reflect = |i: i64, n: i64| -> usize {
        if n == 1 {
            return 0;
        }
        let mut i = i;
        loop {
            if i < 0 {
                i = -i;
            } else if i >= n {
                i = 2 * (n - 1) - i;
            } else {
                return i as usize;
            }
        }
    };
    let (w, h) = (src.width(), src.height());
    // 横方向だけかけた行は、縦方向に要る 2r+1 行ぶんだけを輪のように使い回して持つ (y 行目は y % ring 番目)。
    // 出力の y 行目に要るのは max(0, y-r) 〜 min(h-1, y+r) 行目 (折り返しても範囲内) なので、
    // 1 行ずつ横と縦を交互に進めれば、画像全体の途中の結果をメモリに書いて読み直さずに済む
    let ring = (2 * r + 1).min(h);
    scratch.resize(ring * w, 0.0);
    // 横方向: 両端を折り返した行 (塊がはみ出さないよう後ろに余白) を作り、塊ごとに足し込む
    let mut pad = vec![0f32; w + 2 * r + CH];
    // 折り返しが 1 回で済む (重みの半径が幅より小さい) なら、行はまとめてコピーし、両端だけ折り返す
    let simple = r < w;
    let cols: Vec<usize> = (0..w + 2 * r)
        .map(|i| reflect(i as i64 - r as i64, w as i64))
        .collect();
    let mut horizontal = |y: usize, scratch: &mut [f32]| {
        let row = src.row(y);
        if simple {
            pad[r..r + w].copy_from_slice(row);
            for i in 0..r {
                pad[r - 1 - i] = row[cols[r - 1 - i]];
                pad[r + w + i] = row[cols[r + w + i]];
            }
        } else {
            for (p, &c) in pad.iter_mut().zip(&cols) {
                *p = row[c];
            }
        }
        let slot = y % ring;
        horizontal_row(&pad, k, &mut scratch[slot * w..(slot + 1) * w]);
    };
    // 縦方向: 重みごとに、上下の行を足して行全体へ足す (行をまるごと順に読むので、塊ごとより速い)
    let mut out = Image::new(w, h, 0f32);
    let mut done = 0;
    for y in 0..h {
        while done <= (y + r).min(h - 1) {
            horizontal(done, scratch);
            done += 1;
        }
        let tmp: &[f32] = scratch;
        let row_at = |d: i64| {
            let slot = reflect(y as i64 + d, h as i64) % ring;
            &tmp[slot * w..(slot + 1) * w]
        };
        let out_row = &mut out.data_mut()[y * w..(y + 1) * w];
        for (o, &v) in out_row.iter_mut().zip(row_at(0)) {
            *o = k[r] * v;
        }
        for i in 1..=r {
            let (up, down, kk) = (row_at(-(i as i64)), row_at(i as i64), k[r + i]);
            for ((o, &a), &b) in out_row.iter_mut().zip(up).zip(down) {
                *o += kk * (a + b);
            }
        }
    }
    out
}

/// 横、縦の順に、重み k で畳み込む (float32 で、重みの順に足す)。画像の外は端で折り返す (BORDER_REFLECT_101)。
/// どちらの方向も「重みごとに、行全体へ足す」形にして、コンパイラが SIMD にしやすくしてある
/// (1 画素ごとに足す順番は重みの順のままなので、結果は変わらない)
pub fn sep_filter_reflect101(src: &Image<f32>, k: &[f32]) -> Image<f32> {
    let r = k.len() / 2;
    let reflect = |i: i64, n: i64| -> usize {
        if n == 1 {
            return 0;
        }
        let mut i = i;
        loop {
            if i < 0 {
                i = -i;
            } else if i >= n {
                i = 2 * (n - 1) - i;
            } else {
                return i as usize;
            }
        }
    };
    let (w, h) = (src.width(), src.height());
    // 横方向: 両端を折り返した行を作り、重みごとにずらして足す
    let mut tmp = Image::new(w, h, 0f32);
    let mut pad = vec![0f32; w + 2 * r];
    let cols: Vec<usize> = (0..w + 2 * r)
        .map(|i| reflect(i as i64 - r as i64, w as i64))
        .collect();
    for y in 0..h {
        let row = src.row(y);
        for (p, &c) in pad.iter_mut().zip(&cols) {
            *p = row[c];
        }
        let out = &mut tmp.data_mut()[y * w..(y + 1) * w];
        for (i, &kv) in k.iter().enumerate() {
            for (o, &v) in out.iter_mut().zip(&pad[i..i + w]) {
                *o += kv * v;
            }
        }
    }
    // 縦方向: 重みごとに、折り返した行を足す
    let mut out = Image::new(w, h, 0f32);
    let idx: Vec<Vec<usize>> = (0..h)
        .map(|y| {
            (0..k.len())
                .map(|i| reflect(y as i64 + i as i64 - r as i64, h as i64))
                .collect()
        })
        .collect();
    for (y, taps) in idx.iter().enumerate() {
        let out_row = &mut out.data_mut()[y * w..(y + 1) * w];
        for (&kv, &yy) in k.iter().zip(taps) {
            for (o, &v) in out_row.iter_mut().zip(tmp.row(yy)) {
                *o += kv * v;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{json_fixture, json_image, json_u8};

    /// 対称のぼかし (横と縦を 1 行ずつ交互に進め、途中の結果は 2r+1 行だけ持つ) が、1 画素ずつ「横、縦の順に、
    /// 真ん中の重み + 中心から等しい距離の 2 画素の和 x 重み を近いほうから」足したものと全画素で同じか。
    /// 高さが 2r+1 より小さい画像 (途中の結果が全部の行になる) や、幅が半径より小さい画像 (折り返しが何回も) も試す
    #[test]
    fn symmetric_blur_matches_plain_sum() {
        let refl = |i: i64, n: i64| -> usize {
            if n == 1 {
                return 0;
            }
            let mut i = i;
            loop {
                if i < 0 {
                    i = -i
                } else if i >= n {
                    i = 2 * (n - 1) - i
                } else {
                    return i as usize;
                }
            }
        };
        let mut seed = 1u32;
        let mut scratch = Vec::new();
        for (w, h, sigma) in [
            (61, 47, 1.2262735f64),
            (40, 23, 3.0900643),
            (5, 3, 3.0900643),
            (1, 9, 1.6),
        ] {
            let img = Image::from_vec(
                w,
                h,
                (0..w * h)
                    .map(|_| {
                        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                        (seed >> 24) as f32
                    })
                    .collect(),
            );
            let k = gaussian_kernel(
                ((sigma * 8.0 + 1.0).round_ties_even() as usize) | 1,
                2.0 * sigma * sigma,
            );
            let r = k.len() / 2;
            let got = sep_filter_symmetric_reflect101(&img, &k, &mut scratch);
            let horiz = |x: usize, y: usize| {
                let mut v = k[r] * img.get(x, y);
                for i in 1..=r {
                    v += k[r + i]
                        * (img.get(refl(x as i64 - i as i64, w as i64), y)
                            + img.get(refl(x as i64 + i as i64, w as i64), y));
                }
                v
            };
            for y in 0..h {
                for x in 0..w {
                    let mut v = k[r] * horiz(x, y);
                    for i in 1..=r {
                        v += k[r + i]
                            * (horiz(x, refl(y as i64 - i as i64, h as i64))
                                + horiz(x, refl(y as i64 + i as i64, h as i64)));
                    }
                    assert_eq!(got.get(x, y), v, "{w}x{h} ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn matches_opencv() {
        let f = json_fixture("cv/morph.json");
        let src = json_image(&f["src"], json_u8);
        for k in [5, 25] {
            assert_eq!(
                erode(&src, k),
                json_image(&f[format!("erode{k}")], json_u8),
                "erode{k}"
            );
            assert_eq!(
                dilate(&src, k),
                json_image(&f[format!("dilate{k}")], json_u8),
                "dilate{k}"
            );
            assert_eq!(
                tophat(&src, k),
                json_image(&f[format!("tophat{k}")], json_u8),
                "tophat{k}"
            );
        }
    }
}
