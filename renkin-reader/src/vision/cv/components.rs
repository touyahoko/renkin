//! 連結成分 (8 近傍)
//!
//! OpenCV (Apache License 2.0) の modules/imgproc/src/connectedcomponents.cpp の connectedComponentsWithStats (番号の順番) に合わせて、Rust で書いたもの。
//! ライセンスと著作権表示はリポジトリの THIRD_PARTY_LICENSES/OpenCV を参照

use super::image::Image;

/// 1 つのかたまりの外接長方形と画素数
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Component {
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
    pub area: usize,
    /// 重心 (画素の座標の平均)
    pub cx: f64,
    pub cy: f64,
}

/// true の画素を、上下左右と斜めのつながりでかたまりに分ける。
/// (番号の画像, かたまりの一覧) を返す。番号は 1 から、OpenCV の connectedComponentsWithStats と同じ順
/// (ただし OpenCV の 0 番 (背景) は含めない)。
///
/// OpenCV の既定の方式は 2x2 のブロック単位で左上から見ていくので、番号は「かたまりを含む最初の 2x2 ブロック」
/// (行÷2, 列÷2) の順になる。2x2 のブロックの画素はどれも隣り合うので、1 つのブロックに 2 つのかたまりは入らない
pub fn connected_components(mask: &Image<bool>) -> (Image<u32>, Vec<Component>) {
    let (labels, comps, first_block) = label_in_scan_order(mask);
    // 最初の 2x2 ブロックの順に、番号を付け直す
    let mut order: Vec<usize> = (0..comps.len()).collect();
    order.sort_by_key(|&i| first_block[i]);
    let mut renumber = vec![0u32; comps.len() + 1];
    for (new, &old) in order.iter().enumerate() {
        renumber[old + 1] = new as u32 + 1;
    }
    let data = labels
        .data()
        .iter()
        .map(|&l| renumber[l as usize])
        .collect();
    let labels = Image::from_vec(mask.width(), mask.height(), data);
    (labels, order.iter().map(|&i| comps[i]).collect())
}

/// 左上から 1 画素ずつ見て、かたまりに番号を付ける。かたまりごとに、含む最初の 2x2 ブロック (行÷2, 列÷2) も返す
#[allow(clippy::type_complexity)]
fn label_in_scan_order(mask: &Image<bool>) -> (Image<u32>, Vec<Component>, Vec<(usize, usize)>) {
    let (w, h) = (mask.width(), mask.height());
    let mut labels = Image::new(w, h, 0u32);
    let mut comps = Vec::new();
    let mut first_block = Vec::new();
    let mut stack = Vec::new();
    for y0 in 0..h {
        for x0 in 0..w {
            if !mask.get(x0, y0) || labels.get(x0, y0) != 0 {
                continue;
            }
            let id = comps.len() as u32 + 1;
            let (mut l, mut t, mut r, mut b, mut area) = (x0, y0, x0, y0, 0);
            let mut block = (y0 / 2, x0 / 2);
            let (mut sx, mut sy) = (0usize, 0usize);
            labels.set(x0, y0, id);
            stack.push((x0, y0));
            while let Some((x, y)) = stack.pop() {
                area += 1;
                (l, t, r, b) = (l.min(x), t.min(y), r.max(x), b.max(y));
                block = block.min((y / 2, x / 2));
                (sx, sy) = (sx + x, sy + y);
                for ny in y.saturating_sub(1)..(y + 2).min(h) {
                    for nx in x.saturating_sub(1)..(x + 2).min(w) {
                        if mask.get(nx, ny) && labels.get(nx, ny) == 0 {
                            labels.set(nx, ny, id);
                            stack.push((nx, ny));
                        }
                    }
                }
            }
            let (cx, cy) = (sx as f64 / area as f64, sy as f64 / area as f64);
            comps.push(Component {
                left: l,
                top: t,
                width: r - l + 1,
                height: b - t + 1,
                area,
                cx,
                cy,
            });
            first_block.push(block);
        }
    }
    (labels, comps, first_block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{json_fixture, json_image};

    #[test]
    fn matches_opencv() {
        let f = json_fixture("cv/components.json");
        let mask = json_image(&f["mask"], |v| v.as_u64().unwrap() != 0);
        let (labels, comps) = connected_components(&mask);
        assert_eq!(comps.len() + 1, f["count"].as_u64().unwrap() as usize);
        assert_eq!(
            labels,
            json_image(&f["labels"], |v| v.as_u64().unwrap() as u32)
        );
        for (c, s) in comps
            .iter()
            .zip(f["stats"].as_array().unwrap().iter().skip(1))
        {
            let s: Vec<usize> = s
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as usize)
                .collect();
            assert_eq!(
                [c.left, c.top, c.width, c.height, c.area],
                [s[0], s[1], s[2], s[3], s[4]]
            );
        }
    }

    #[test]
    fn centroids_match_opencv() {
        let f = json_fixture("cv/align_ops.json");
        let (_, comps) =
            connected_components(&json_image(&f["cc_mask"], |v| v.as_u64().unwrap() != 0));
        for (c, w) in comps.iter().zip(f["cc_centroids"].as_array().unwrap()) {
            assert!(
                (c.cx - w[0].as_f64().unwrap()).abs() < 1e-12
                    && (c.cy - w[1].as_f64().unwrap()).abs() < 1e-12
            );
        }
    }
}
