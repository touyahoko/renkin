//! SIFT (特徴点と記述子)。
//!
//! OpenCV (Apache License 2.0) の modules/features/src/sift.dispatch.cpp と sift.simd.hpp を NumPy でなぞった
//! experimental-charm-reader の pure_sift.py を、Rust に移したもの。float32 と float64 の使い分け (NumPy の型の
//! 上がり方を含む) も pure_sift.py と同じにしてある。ライセンスと著作権表示はリポジトリの THIRD_PARTY_LICENSES/OpenCV を参照

use super::filter::{gaussian_kernel, sep_filter_reflect101, sep_filter_symmetric_reflect101};
use super::image::Image;
use super::linalg::{Mat3, solve3};

const SIFT_DESCR_WIDTH: usize = 4;
const SIFT_DESCR_HIST_BINS: usize = 8;
const SIFT_INIT_SIGMA: f64 = 0.5;
const SIFT_IMG_BORDER: i64 = 5;
const SIFT_MAX_INTERP_STEPS: usize = 5;
const SIFT_ORI_HIST_BINS: usize = 36;
const SIFT_ORI_SIG_FCTR: f64 = 1.5;
const SIFT_ORI_RADIUS: f64 = 3.0 * SIFT_ORI_SIG_FCTR;
const SIFT_ORI_PEAK_RATIO: f32 = 0.8;
const SIFT_DESCR_SCL_FCTR: f64 = 3.0;
const SIFT_DESCR_MAG_THR: f32 = 0.2;
const SIFT_INT_DESCR_FCTR: f32 = 512.0;
const FLT_EPSILON: f32 = f32::EPSILON;

/// 記述子の長さ (4 x 4 の区画 x 8 方向)
pub const DESCRIPTOR_LEN: usize = SIFT_DESCR_WIDTH * SIFT_DESCR_WIDTH * SIFT_DESCR_HIST_BINS;

/// 特徴点 (OpenCV の cv::KeyPoint と同じ中身)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyPoint {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub angle: f32,
    pub response: f32,
    /// 下位 8 ビットがオクターブ (負は 2 の補数)、次の 8 ビットが層、その上が層の中の位置
    pub octave: i32,
}

/// cvRound (偶数への丸め)
fn cv_round(v: f64) -> i64 {
    v.round_ties_even() as i64
}

/// 灰色の画像を 2 倍に拡大する (cv::resize の線形補間。元の座標 = 出力 / 2 - 0.25、端は端の値)
fn upscale2x_linear(img: &Image<f32>) -> Image<f32> {
    let up_x = |src: &Image<f32>| -> Image<f32> {
        let (w, h) = (src.width(), src.height());
        let mut out = Image::new(2 * w, h, 0f32);
        for y in 0..h {
            let row = src.row(y);
            for x in 0..w {
                let prev = row[x.saturating_sub(1)];
                let cur = row[x];
                let next = row[(x + 1).min(w - 1)];
                out.set(2 * x, y, prev * 0.25 + cur * 0.75);
                out.set(2 * x + 1, y, cur * 0.75 + next * 0.25);
            }
        }
        out
    };
    let up_y = |src: &Image<f32>| -> Image<f32> {
        let (w, h) = (src.width(), src.height());
        let mut out = Image::new(w, 2 * h, 0f32);
        for y in 0..h {
            let (prev, cur, next) = (
                src.row(y.saturating_sub(1)),
                src.row(y),
                src.row((y + 1).min(h - 1)),
            );
            for x in 0..w {
                out.set(x, 2 * y, prev[x] * 0.25 + cur[x] * 0.75);
                out.set(x, 2 * y + 1, cur[x] * 0.75 + next[x] * 0.25);
            }
        }
        out
    };
    up_y(&up_x(img))
}

/// 半分の大きさにする (cv::resize の最近傍。元の座標 = floor(出力 x (元の長さ / 出力の長さ)))
fn downsample_nearest(img: &Image<f32>) -> Image<f32> {
    let (w, h) = (img.width(), img.height());
    let (dw, dh) = (w / 2, h / 2);
    let xs: Vec<usize> = (0..dw)
        .map(|x| ((x as f64 * (w as f64 / dw as f64)).floor() as usize).min(w - 1))
        .collect();
    let mut out = Image::new(dw, dh, 0f32);
    for y in 0..dh {
        let sy = ((y as f64 * (h as f64 / dh as f64)).floor() as usize).min(h - 1);
        let row = img.row(sy);
        for (x, &sx) in xs.iter().enumerate() {
            out.set(x, y, row[sx]);
        }
    }
    out
}

/// ぼかし。sigma が float32 のとき (最初の画像) と float64 のとき (ピラミッド) で、NumPy 版の重みの作り方が違う
enum Sigma {
    F32(f32),
    F64(f64),
}

fn gaussian_blur(
    img: &Image<f32>,
    sigma: Sigma,
    symmetric: bool,
    scratch: &mut Vec<f32>,
) -> Image<f32> {
    let (ksize, denom) = match sigma {
        // NumPy 2 では float32 と Python の数の計算は float32 のまま進む
        Sigma::F32(s) => (
            (cv_round((s * 4.0 * 2.0 + 1.0) as f64) | 1) as usize,
            ((2.0f32 * s) * s) as f64,
        ),
        Sigma::F64(s) => ((cv_round(s * 4.0 * 2.0 + 1.0) | 1) as usize, 2.0 * s * s),
    };
    let k = gaussian_kernel(ksize, denom);
    if symmetric {
        sep_filter_symmetric_reflect101(img, &k, scratch)
    } else {
        sep_filter_reflect101(img, &k)
    }
}

const P1: f32 = (0.9997878412794807 * 180.0 / std::f64::consts::PI) as f32;
const P3: f32 = (-0.3258083974640975 * 180.0 / std::f64::consts::PI) as f32;
const P5: f32 = (0.1555786518463281 * 180.0 / std::f64::consts::PI) as f32;
const P7: f32 = (-0.04432655554792128 * 180.0 / std::f64::consts::PI) as f32;
const DBL_EPS_F: f32 = f64::EPSILON as f32;

/// 閾値を超え、同じ層の上下左右 4 近傍の全部以上 (正) か全部以下 (負) の画素に 1 を立てる。
/// 端の画素は使わないので計算しない。`&` で分岐を作らないので SIMD になる
#[inline(never)]
fn candidates(up: &[f32], center: &[f32], down: &[f32], threshold: f32, mask: &mut [u8]) {
    let n = center.len();
    if n < 3 {
        return;
    }
    let (l, c, r, u, d) = (
        &center[..n - 2],
        &center[1..n - 1],
        &center[2..],
        &up[1..n - 1],
        &down[1..n - 1],
    );
    for (i, m) in mask[1..n - 1].iter_mut().enumerate() {
        let (v, l, r, u, d) = (c[i], l[i], r[i], u[i], d[i]);
        let pos = (v > threshold) & (v >= l) & (v >= r) & (v >= u) & (v >= d);
        let neg = (v < -threshold) & (v <= l) & (v <= r) & (v <= u) & (v <= d);
        *m = (pos | neg) as u8;
    }
}

/// cv::hal::fastAtan2 (度。多項式の近似、float32)
fn fast_atan2(y: f32, x: f32) -> f32 {
    let (ax, ay) = (x.abs(), y.abs());
    let big = ax >= ay;
    let c = if big {
        ay / (ax + DBL_EPS_F)
    } else {
        ax / (ay + DBL_EPS_F)
    };
    let c2 = c * c;
    let poly = (((P7 * c2 + P5) * c2 + P3) * c2 + P1) * c;
    let mut a = if big { poly } else { 90.0 - poly };
    if x < 0.0 {
        a = 180.0 - a;
    }
    if y < 0.0 {
        a = 360.0 - a;
    }
    a
}

pub struct Sift {
    pub nfeatures: usize,
    /// ぼかしを左右対称の畳み込み (速い) でするか、NumPy 版と同じ順番で足すか
    pub symmetric_blur: bool,
    n_layers: usize,
    contrast_threshold: f64,
    edge_threshold: f64,
    sigma: f64,
}

impl Sift {
    /// OpenCV の SIFT_create(nfeatures) と同じ既定値 (層 3、コントラスト 0.04、エッジ 10、sigma 1.6、最初に 2 倍に拡大)
    pub fn new(nfeatures: usize) -> Self {
        Self {
            nfeatures,
            symmetric_blur: true,
            n_layers: 3,
            contrast_threshold: 0.04,
            edge_threshold: 10.0,
            sigma: 1.6,
        }
    }

    /// createInitialImage: 2 倍に拡大して、足りない分のぼかしをかける
    fn initial_image(&self, gray: &Image<u8>) -> Image<f32> {
        let sig_diff = (self.sigma.powi(2) - SIFT_INIT_SIGMA.powi(2) * 4.0)
            .max(0.01)
            .sqrt();
        gaussian_blur(
            &upscale2x_linear(&gray.map(f32::from)),
            Sigma::F32(sig_diff as f32),
            self.symmetric_blur,
            &mut Vec::new(),
        )
    }

    /// buildGaussianPyramid の各層で足すぼかし
    fn octave_sigmas(&self) -> Vec<f64> {
        let nl = self.n_layers;
        let k = 2f64.powf(1.0 / nl as f64);
        let mut sig = vec![self.sigma];
        for i in 1..nl + 3 {
            let prev = k.powi(i as i32 - 1) * self.sigma;
            let total = prev * k;
            sig.push((total * total - prev * prev).sqrt());
        }
        sig
    }

    /// buildGaussianPyramid の 1 段分 (nl + 3 層)。base がその段の 0 層目
    fn gaussian_octave(
        &self,
        base: Image<f32>,
        sig: &[f64],
        scratch: &mut Vec<f32>,
    ) -> Vec<Image<f32>> {
        let mut octave = Vec::with_capacity(self.n_layers + 3);
        octave.push(base);
        for s in &sig[1..] {
            let img = gaussian_blur(
                octave.last().unwrap(),
                Sigma::F64(*s),
                self.symmetric_blur,
                scratch,
            );
            octave.push(img);
        }
        octave
    }

    /// buildDoGPyramid の 1 段分。あとで使うのは 1〜nl 層 (向きと記述子。nl 層は次の段の元にもなる) だけなので、
    /// それを返し、ほかの層 (0 層と nl+1、nl+2 層) は捨てる。捨てる層の領域にはそのまま差を書いて、メモリを取り直さない
    /// (差の計算は 上の層 - 下の層 のままなので、結果は変わらない)
    fn difference_of_gaussians(
        &self,
        octave: Vec<Image<f32>>,
    ) -> (Vec<Image<f32>>, Vec<Image<f32>>) {
        let kept = 1..=self.n_layers;
        let mut layers: Vec<Option<Image<f32>>> = octave.into_iter().map(Some).collect();
        let mut dog = Vec::with_capacity(layers.len() - 1);
        for i in 0..layers.len() - 1 {
            let d = if kept.contains(&i) {
                let (lower, upper) = (layers[i].as_ref().unwrap(), layers[i + 1].as_ref().unwrap());
                let data = upper
                    .data()
                    .iter()
                    .zip(lower.data())
                    .map(|(a, b)| a - b)
                    .collect();
                Image::from_vec(lower.width(), lower.height(), data)
            } else {
                // 下の層はこの差を作ったらもう使わない
                let mut lower = layers[i].take().unwrap();
                let upper = layers[i + 1].as_ref().unwrap();
                for (b, a) in lower.data_mut().iter_mut().zip(upper.data()) {
                    *b = a - *b;
                }
                lower
            };
            dog.push(d);
        }
        let kept = layers
            .into_iter()
            .enumerate()
            .filter(|(i, _)| kept.contains(i))
            .map(|(_, l)| l.unwrap())
            .collect();
        (kept, dog)
    }

    /// adjustLocalExtrema: 極値の位置を補間で詰め、弱いものや辺の上のものを捨てる。(特徴点, 層, 行, 列) を返す
    fn adjust(
        &self,
        dog: &[Image<f32>],
        o: usize,
        layer: usize,
        r: usize,
        c: usize,
    ) -> Option<(KeyPoint, usize, usize, usize)> {
        let nl = self.n_layers as i64;
        let img_scale = 1.0 / 255.0;
        let (d1, d2, dc) = (img_scale * 0.5, img_scale, img_scale * 0.25);
        let (w, h) = (dog[0].width() as i64, dog[0].height() as i64);
        let (mut layer, mut r, mut c) = (layer as i64, r as i64, c as i64);
        let (mut xi, mut xr, mut xc) = (0.0, 0.0, 0.0);
        let derivs = |layer: i64, r: i64, c: i64| {
            let g = |l: i64, y: i64, x: i64| dog[l as usize].get(x as usize, y as usize) as f64;
            let (im, pv, nx) = (layer, layer - 1, layer + 1);
            let dd = [
                (g(im, r, c + 1) - g(im, r, c - 1)) * d1,
                (g(im, r + 1, c) - g(im, r - 1, c)) * d1,
                (g(nx, r, c) - g(pv, r, c)) * d1,
            ];
            let v2 = g(im, r, c) * 2.0;
            let dxx = (g(im, r, c + 1) + g(im, r, c - 1) - v2) * d2;
            let dyy = (g(im, r + 1, c) + g(im, r - 1, c) - v2) * d2;
            let dss = (g(nx, r, c) + g(pv, r, c) - v2) * d2;
            let dxy = (g(im, r + 1, c + 1) - g(im, r + 1, c - 1) - g(im, r - 1, c + 1)
                + g(im, r - 1, c - 1))
                * dc;
            let dxs = (g(nx, r, c + 1) - g(nx, r, c - 1) - g(pv, r, c + 1) + g(pv, r, c - 1)) * dc;
            let dys = (g(nx, r + 1, c) - g(nx, r - 1, c) - g(pv, r + 1, c) + g(pv, r - 1, c)) * dc;
            (dd, dxx, dyy, dss, dxy, dxs, dys)
        };
        let mut i = 0;
        while i < SIFT_MAX_INTERP_STEPS {
            let (dd, dxx, dyy, dss, dxy, dxs, dys) = derivs(layer, r, c);
            let hm: Mat3 = [[dxx, dxy, dxs], [dxy, dyy, dys], [dxs, dys, dss]];
            let x = solve3(&hm, &dd).unwrap_or([0.0; 3]);
            (xi, xr, xc) = (-x[2], -x[1], -x[0]);
            if xi.abs() < 0.5 && xr.abs() < 0.5 && xc.abs() < 0.5 {
                break;
            }
            let lim = (i32::MAX as f64) / 3.0;
            if xi.abs() > lim || xr.abs() > lim || xc.abs() > lim {
                return None;
            }
            c += cv_round(xc);
            r += cv_round(xr);
            layer += cv_round(xi);
            if layer < 1
                || layer > nl
                || c < SIFT_IMG_BORDER
                || c >= w - SIFT_IMG_BORDER
                || r < SIFT_IMG_BORDER
                || r >= h - SIFT_IMG_BORDER
            {
                return None;
            }
            i += 1;
        }
        if i >= SIFT_MAX_INTERP_STEPS {
            return None;
        }
        let (dd, dxx, dyy, _, dxy, _, _) = derivs(layer, r, c);
        let t = dd[0] * xc + dd[1] * xr + dd[2] * xi;
        let contr = dog[layer as usize].get(c as usize, r as usize) as f64 * img_scale + t * 0.5;
        if contr.abs() * nl as f64 >= self.contrast_threshold {
            let tr = dxx + dyy;
            let det = dxx * dyy - dxy * dxy;
            let e = self.edge_threshold;
            if det <= 0.0 || tr * tr * e >= (e + 1.0).powi(2) * det {
                return None;
            }
        } else {
            return None;
        }
        let s = (1u32 << o) as f64;
        let kp = KeyPoint {
            x: ((c as f64 + xc) * s) as f32,
            y: ((r as f64 + xr) * s) as f32,
            size: (self.sigma * 2f64.powf((layer as f64 + xi) / nl as f64) * s * 2.0) as f32,
            angle: 0.0,
            response: contr.abs() as f32,
            octave: o as i32
                + ((layer as i32) << 8)
                + ((cv_round((xi + 0.5) * 255.0) as i32) << 16),
        };
        Some((kp, layer as usize, r as usize, c as usize))
    }

    /// calcOrientationHist: (px, py) のまわりの勾配の向きのヒストグラム (36 本、なめらかにしたもの)
    fn orientation_hist(
        img: &Image<f32>,
        px: i64,
        py: i64,
        radius: i64,
        sigma: f64,
    ) -> [f32; SIFT_ORI_HIST_BINS] {
        let n = SIFT_ORI_HIST_BINS;
        let (rows, cols) = (img.height() as i64, img.width() as i64);
        let expf_scale = -1.0 / (2.0 * sigma * sigma);
        let mut t = [0f64; SIFT_ORI_HIST_BINS];
        // 重みは中心からの距離だけで決まるので、(|ii|, |jj|) ごとに 1 回だけ求める
        let side = radius.max(0) as usize + 1;
        let weights: Vec<f32> = (0..side * side)
            .map(|k| {
                let (i, j) = ((k / side) as i64, (k % side) as i64);
                (((i * i + j * j) as f64 * expf_scale) as f32).exp()
            })
            .collect();
        for ii in -radius..=radius {
            let y = py + ii;
            if !(y > 0 && y < rows - 1) {
                continue;
            }
            let (up, mid, down) = (
                img.row(y as usize - 1),
                img.row(y as usize),
                img.row(y as usize + 1),
            );
            let wrow = &weights[ii.unsigned_abs() as usize * side..][..side];
            for jj in -radius..=radius {
                let x = px + jj;
                if !(x > 0 && x < cols - 1) {
                    continue;
                }
                let x = x as usize;
                let dx = mid[x + 1] - mid[x - 1];
                let dy = up[x] - down[x];
                let w = wrow[jj.unsigned_abs() as usize];
                let ori = fast_atan2(dy, dx);
                let mag = (dx * dx + dy * dy).sqrt();
                let mut b = ((n as f64 / 360.0) as f32 * ori).round_ties_even() as i64;
                if b >= n as i64 {
                    b -= n as i64;
                }
                if b < 0 {
                    b += n as i64;
                }
                t[b as usize] += (w * mag) as f64;
            }
        }
        let t: Vec<f32> = t.iter().map(|&v| v as f32).collect();
        let mut hist = [0f32; SIFT_ORI_HIST_BINS];
        for j in 0..n {
            let at = |k: i64| t[((j as i64 + k).rem_euclid(n as i64)) as usize];
            hist[j] = (at(-2) + at(2)) * (1.0 / 16.0) as f32
                + (at(-1) + at(1)) * (4.0 / 16.0) as f32
                + at(0) * (6.0 / 16.0) as f32;
        }
        hist
    }

    /// findScaleSpaceExtrema の 1 段 (o 段) 分。gauss はその段の 1〜nl 層、dog はその段の差の全部。見つけた特徴点を kps に足す
    fn octave_extrema(
        &self,
        o: usize,
        gauss: &[Image<f32>],
        dog: &[Image<f32>],
        kps: &mut Vec<KeyPoint>,
    ) {
        let nl = self.n_layers;
        let n = SIFT_ORI_HIST_BINS;
        let threshold = (0.5 * self.contrast_threshold / nl as f64 * 255.0).floor() as f32;
        let b = SIFT_IMG_BORDER as usize;
        let (w, h) = (dog[0].width(), dog[0].height());
        if h <= 2 * b || w <= 2 * b {
            return;
        }
        for i in 1..=nl {
            let (prev, cur, next) = (&dog[i - 1], &dog[i], &dog[i + 1]);
            let mut mask = vec![0u8; w];
            for r in b..h - b {
                // 前後の層と同じ層の、上・中・下の行
                let rows: [&[f32]; 9] = [prev, cur, next]
                    .map(|im| [im.row(r - 1), im.row(r), im.row(r + 1)])
                    .concat()
                    .try_into()
                    .unwrap();
                // 閾値と同じ層の上下左右だけで、行全体を分岐なしにふるいにかける (SIMD にしやすい)
                candidates(rows[3], rows[4], rows[5], threshold, &mut mask);
                for c in b..w - b {
                    if mask[c] == 0 {
                        continue;
                    }
                    let v = rows[4][c];
                    // 26 近傍の全部以上 (正のとき) か、全部以下 (負のとき) か。違うとわかったら打ち切る
                    let ok = |x: f32| if v > 0.0 { x <= v } else { x >= v };
                    let is_extremum = rows.iter().enumerate().all(|(i, row)| {
                        let n = &row[c - 1..c + 2];
                        if i == 4 {
                            ok(n[0]) && ok(n[2])
                        } else {
                            ok(n[0]) && ok(n[1]) && ok(n[2])
                        }
                    });
                    if !is_extremum {
                        continue;
                    }
                    let Some((kp, layer, r1, c1)) = self.adjust(dog, o, i, r, c) else {
                        continue;
                    };
                    let scl_octv = kp.size as f64 * 0.5 / (1u32 << o) as f64;
                    let hist = Self::orientation_hist(
                        &gauss[layer - 1],
                        c1 as i64,
                        r1 as i64,
                        cv_round(SIFT_ORI_RADIUS * scl_octv),
                        SIFT_ORI_SIG_FCTR * scl_octv,
                    );
                    let mag_thr = hist.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                        * SIFT_ORI_PEAK_RATIO;
                    for j in 0..n {
                        let (hl, hj, hr) = (hist[(j + n - 1) % n], hist[j], hist[(j + 1) % n]);
                        if hj > hl && hj > hr && hj >= mag_thr {
                            let mut bin = j as f32 + 0.5 * (hl - hr) / (hl - 2.0 * hj + hr);
                            if bin < 0.0 {
                                bin += n as f32;
                            } else if bin >= n as f32 {
                                bin -= n as f32;
                            }
                            let mut ang = 360.0 - (360.0 / n as f32) * bin;
                            if (ang - 360.0).abs() < FLT_EPSILON {
                                ang = 0.0;
                            }
                            kps.push(KeyPoint { angle: ang, ..kp });
                        }
                    }
                }
            }
        }
    }

    /// calcSIFTDescriptor
    fn descriptor(
        img: &Image<f32>,
        ptx: f64,
        pty: f64,
        ori: f64,
        scl: f64,
    ) -> [f32; DESCRIPTOR_LEN] {
        let (d, n) = (SIFT_DESCR_WIDTH, SIFT_DESCR_HIST_BINS);
        let (rows, cols) = (img.height() as i64, img.width() as i64);
        let (px, py) = (cv_round(ptx), cv_round(pty));
        let rad = ori * std::f64::consts::PI / 180.0;
        let hist_width = (SIFT_DESCR_SCL_FCTR * scl) as f32;
        let cos_t = rad.cos() as f32 / hist_width;
        let sin_t = rad.sin() as f32 / hist_width;
        let bins_per_rad = (n as f64 / 360.0) as f32;
        let exp_scale = (-1.0 / (d as f64 * d as f64 * 0.5)) as f32;
        let radius =
            cv_round((hist_width * std::f32::consts::SQRT_2 * (d as f32 + 1.0) * 0.5) as f64);
        let radius = radius.min(((cols as f64).powi(2) + (rows as f64).powi(2)).sqrt() as i64);
        let half = (d as f64 / 2.0 - 0.5) as f32;
        let mut hist = vec![0f64; (d + 2) * (d + 2) * (n + 2)];
        // OpenCV と同じく、範囲内の標本を先に集めてから、角度・重み・大きさを配列でまとめて求める
        let cap = ((2 * radius + 1) * (2 * radius + 1)) as usize;
        let (mut gx, mut gy, mut wa, mut rbs, mut cbs) = (
            Vec::with_capacity(cap),
            Vec::with_capacity(cap),
            Vec::with_capacity(cap),
            Vec::with_capacity(cap),
            Vec::with_capacity(cap),
        );
        for ii in -radius..=radius {
            let r = py + ii;
            if !(r > 0 && r < rows - 1) {
                continue;
            }
            let (up, mid, down) = (
                img.row(r as usize - 1),
                img.row(r as usize),
                img.row(r as usize + 1),
            );
            for jj in -radius..=radius {
                let (fi, fj) = (ii as f32, jj as f32);
                let c_rot = fj * cos_t - fi * sin_t;
                let r_rot = fj * sin_t + fi * cos_t;
                let rbin = r_rot + half;
                let cbin = c_rot + half;
                let c = px + jj;
                if !(rbin > -1.0
                    && rbin < d as f32
                    && cbin > -1.0
                    && cbin < d as f32
                    && c > 0
                    && c < cols - 1)
                {
                    continue;
                }
                let c = c as usize;
                gx.push(mid[c + 1] - mid[c - 1]);
                gy.push(up[c] - down[c]);
                wa.push((c_rot * c_rot + r_rot * r_rot) * exp_scale);
                rbs.push(rbin);
                cbs.push(cbin);
            }
        }
        let oris: Vec<f32> = gx
            .iter()
            .zip(&gy)
            .map(|(&dx, &dy)| fast_atan2(dy, dx))
            .collect();
        let mags: Vec<f32> = gx
            .iter()
            .zip(&gy)
            .zip(&wa)
            .map(|((&dx, &dy), &a)| (dx * dx + dy * dy).sqrt() * a.exp())
            .collect();
        for k in 0..gx.len() {
            let (rbin, cbin, ori_px, mag) = (rbs[k], cbs[k], oris[k], mags[k]);
            let obin = (ori_px - ori as f32) * bins_per_rad;
            let (r0, c0, mut o0) = (
                rbin.floor() as i64,
                cbin.floor() as i64,
                obin.floor() as i64,
            );
            // NumPy 版では「float32 の配列 - 整数の配列」で float64 に上がり、ここから先は float64
            let (rb, cb, ob) = (
                rbin as f64 - r0 as f64,
                cbin as f64 - c0 as f64,
                obin as f64 - o0 as f64,
            );
            if o0 < 0 {
                o0 += n as i64;
            }
            if o0 >= n as i64 {
                o0 -= n as i64;
            }
            let mag = mag as f64;
            let v_r1 = mag * rb;
            let v_r0 = mag - v_r1;
            let v_rc11 = v_r1 * cb;
            let v_rc10 = v_r1 - v_rc11;
            let v_rc01 = v_r0 * cb;
            let v_rc00 = v_r0 - v_rc01;
            let v111 = v_rc11 * ob;
            let v110 = v_rc11 - v111;
            let v101 = v_rc10 * ob;
            let v100 = v_rc10 - v101;
            let v011 = v_rc01 * ob;
            let v010 = v_rc01 - v011;
            let v001 = v_rc00 * ob;
            let v000 = v_rc00 - v001;
            let idx = (((r0 + 1) * (d as i64 + 2) + c0 + 1) * (n as i64 + 2) + o0) as usize;
            let (row, plane) = (n + 2, (d + 2) * (n + 2));
            hist[idx] += v000;
            hist[idx + 1] += v001;
            hist[idx + row] += v010;
            hist[idx + row + 1] += v011;
            hist[idx + plane] += v100;
            hist[idx + plane + 1] += v101;
            hist[idx + plane + row] += v110;
            hist[idx + plane + row + 1] += v111;
        }
        let mut raw = [0f32; DESCRIPTOR_LEN];
        for i in 0..d {
            for j in 0..d {
                let base = ((i + 1) * (d + 2) + (j + 1)) * (n + 2);
                let mut h: Vec<f32> = (0..n + 2).map(|k| hist[base + k] as f32).collect();
                h[0] += h[n];
                h[1] += h[n + 1];
                raw[(i * d + j) * n..(i * d + j + 1) * n].copy_from_slice(&h[..n]);
            }
        }
        let dot = |v: &[f32]| v.iter().fold(0f32, |a, &x| a + x * x);
        let thr = ((dot(&raw) as f64).sqrt() as f32) * SIFT_DESCR_MAG_THR;
        for v in raw.iter_mut() {
            *v = v.min(thr);
        }
        let nrm = (SIFT_INT_DESCR_FCTR as f64 / ((dot(&raw) as f64).sqrt()).max(FLT_EPSILON as f64))
            as f32;
        for v in raw.iter_mut() {
            *v = (*v * nrm).round_ties_even().clamp(0.0, 255.0);
        }
        raw
    }

    /// 特徴点と記述子 (特徴点ごとに 128 個) を求める
    pub fn detect_and_compute(
        &self,
        gray: &Image<u8>,
    ) -> (Vec<KeyPoint>, Vec<[f32; DESCRIPTOR_LEN]>) {
        let first_octave: i32 = -1;
        use crate::timing::time;
        let base = time("sift.initial", || self.initial_image(gray));
        let n_oct = (cv_round((base.width().min(base.height()) as f64).ln() / 2f64.ln() - 2.0)
            - first_octave as i64) as usize;
        // 段ごとに、ぼかした画像を作る → 差を作る → 極値を探す、を済ませてから次の段に進む。
        // 全部の段を作ってから探すより、一度に持つ画像が少なくて済む (いちばん大きい 0 段は、2 倍に拡大した画像の大きさ)。
        // gpyr は各段の 1〜nl 層だけを持つ (向きと記述子で使う)
        let nl = self.n_layers;
        let sig = self.octave_sigmas();
        let mut scratch = Vec::new(); // ぼかしの途中の結果。ぼかしのたびに使い回す
        let mut gpyr: Vec<Vec<Image<f32>>> = Vec::with_capacity(n_oct);
        let mut kps = Vec::new();
        let mut base = Some(base);
        for o in 0..n_oct {
            // 次の段の 0 層は、前の段の nl 層を半分にしたもの
            let first = base
                .take()
                .unwrap_or_else(|| downsample_nearest(&gpyr[o - 1][nl - 1]));
            let octave = time("sift.pyramid", || {
                self.gaussian_octave(first, &sig, &mut scratch)
            });
            let (gauss, dog) = time("sift.dog", || self.difference_of_gaussians(octave));
            time("sift.extrema", || {
                self.octave_extrema(o, &gauss, &dog, &mut kps)
            });
            gpyr.push(gauss);
        }
        drop(scratch);

        // KeyPointsFilter::removeDuplicatedSorted
        kps.sort_by(|a, b| {
            a.x.total_cmp(&b.x)
                .then(a.y.total_cmp(&b.y))
                .then(b.size.total_cmp(&a.size))
                .then(a.angle.total_cmp(&b.angle))
                .then(b.response.total_cmp(&a.response))
                .then(b.octave.cmp(&a.octave))
        });
        kps.dedup_by(|k, prev| {
            (k.x, k.y, k.size, k.angle) == (prev.x, prev.y, prev.size, prev.angle)
        });

        // KeyPointsFilter::retainBest
        if self.nfeatures > 0 && self.nfeatures < kps.len() {
            let mut resp: Vec<f32> = kps.iter().map(|k| k.response).collect();
            resp.sort_by(|a, b| b.total_cmp(a));
            let thr = resp[self.nfeatures - 1];
            kps.retain(|k| k.response >= thr);
        }

        // 2 倍に拡大した画像の座標から、入力の座標に戻す
        let s = 1.0 / (1u32 << -first_octave) as f64;
        for k in kps.iter_mut() {
            k.octave = (k.octave & !255) | ((k.octave + first_octave) & 255);
            k.x = (k.x as f64 * s) as f32;
            k.y = (k.y as f64 * s) as f32;
            k.size = (k.size as f64 * s) as f32;
        }

        let des = time("sift.descriptors", || {
            kps.iter()
                .map(|k| {
                    let mut octave = k.octave & 255;
                    let layer = ((k.octave >> 8) & 255) as usize;
                    if octave >= 128 {
                        octave -= 256;
                    }
                    let scale = if octave >= 0 {
                        1.0 / (1u32 << octave) as f64
                    } else {
                        (1u32 << -octave) as f64
                    };
                    let img = &gpyr[(octave - first_octave) as usize][layer - 1];
                    let mut ang = 360.0 - k.angle as f64;
                    if (ang - 360.0).abs() < FLT_EPSILON as f64 {
                        ang = 0.0;
                    }
                    Self::descriptor(
                        img,
                        k.x as f64 * scale,
                        k.y as f64 * scale,
                        ang,
                        k.size as f64 * scale * 0.5,
                    )
                })
                .collect()
        });
        (kps, des)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{json_fixture, load_png_rgba};

    /// 型紙 (assets/reference.png) の灰色の画素
    fn reference() -> Image<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/reference.png");
        let (rgba, w, h) = load_png_rgba(&path);
        Image::from_vec(
            w as usize,
            h as usize,
            rgba.chunks(4).map(|p| p[0]).collect(),
        )
    }

    /// tests/fixtures/sift/ の特徴点 [x, y, 大きさ, 向き] と記述子 (16 進の文字列)
    fn load(name: &str) -> (Vec<[f64; 4]>, Vec<[f32; DESCRIPTOR_LEN]>) {
        let f = json_fixture(&format!("sift/{name}.json"));
        let kps = f["kps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| std::array::from_fn(|i| k[i].as_f64().unwrap()))
            .collect();
        let des = f["des"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| {
                let hex = d.as_str().unwrap();
                std::array::from_fn(|i| {
                    u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap() as f32
                })
            })
            .collect();
        (kps, des)
    }

    /// (相手の特徴点のうち位置・大きさ・向きが tol 以内のものがある数, そのうち記述子が完全に同じ数, 記述子の差の最大)。
    /// tol は (位置と大きさ, 向き)
    fn agreement(
        kps: &[KeyPoint],
        des: &[[f32; DESCRIPTOR_LEN]],
        want: &(Vec<[f64; 4]>, Vec<[f32; DESCRIPTOR_LEN]>),
        tol: (f64, f64),
    ) -> (usize, usize, f32) {
        let (mut matched, mut equal, mut maxdiff) = (0, 0, 0f32);
        for (w, wd) in want.0.iter().zip(&want.1) {
            if let Some(j) = kps.iter().position(|k| {
                (k.x as f64 - w[0]).abs() < tol.0
                    && (k.y as f64 - w[1]).abs() < tol.0
                    && (k.size as f64 - w[2]).abs() < tol.0
                    && (k.angle as f64 - w[3]).abs() < tol.1
            }) {
                matched += 1;
                let d = des[j]
                    .iter()
                    .zip(wd)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0f32, f32::max);
                maxdiff = maxdiff.max(d);
                equal += (d == 0.0) as usize;
            }
        }
        (matched, equal, maxdiff)
    }

    /// NumPy 版 (pure_sift.py) と同じ足し算の順番 (対称にしないぼかし) なら、特徴点も記述子も完全に同じ
    #[test]
    fn reference_matches_numpy() {
        let want = load("numpy");
        let sift = Sift {
            symmetric_blur: false,
            ..Sift::new(8000)
        };
        let (kps, des) = sift.detect_and_compute(&reference());
        let (m, e, d) = agreement(&kps, &des, &want, (1e-3, 1e-2));
        println!(
            "特徴点 {} (NumPy {})、同じ特徴点 {m}、記述子も同じ {e}、記述子の差の最大 {d}",
            kps.len(),
            want.0.len()
        );
        assert_eq!(
            (kps.len(), m, e),
            (want.0.len(), want.0.len(), want.0.len())
        );
    }

    /// OpenCV (Python 版の位置合わせが使うもの) と比べる。本番の設定 (対称のぼかし) で、特徴点は全部同じ、
    /// 記述子は 1402 / 1415 が同じ (OpenCV のぼかしは Intel IPP で、足し算の順番が少し違う)。これより減ったら失敗
    #[test]
    fn reference_close_to_opencv() {
        let want = load("opencv");
        let n = want.0.len();
        for symmetric in [false, true] {
            let (kps, des) = Sift {
                symmetric_blur: symmetric,
                ..Sift::new(8000)
            }
            .detect_and_compute(&reference());
            let (m, e, d) = agreement(&kps, &des, &want, (1e-2, 0.1));
            println!(
                "{}: 特徴点 {} (OpenCV {n})、同じ特徴点 {m}、記述子も同じ {e}、記述子の差の最大 {d}",
                if symmetric {
                    "対称のぼかし"
                } else {
                    "NumPy 版と同じ順番"
                },
                kps.len()
            );
            if symmetric {
                assert_eq!((kps.len(), m), (n, n));
                assert!(e >= 1402 && d <= 1.0, "記述子が OpenCV から離れた");
            }
        }
    }
}
