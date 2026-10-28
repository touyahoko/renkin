//! MHXX の乱数 (xorshift128) と、任意の位置へ一気に飛ぶ jump。
//!
//! apmnnn/mhxx-rng (https://github.com/apmnnn/mhxx-rng) からの移植 (mhxx-combo-scan の combo-core/src/rng.rs を
//! そのまま使い、1 つ戻す descend を足したもの)。
//!
//! ---------------------------------------------------------------------------
//! Original work: MIT License
//!
//! Copyright (c) 2026 apmnnn
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.
//! ---------------------------------------------------------------------------

/// 乱数の初期値
pub const SEED: [u32; 4] = [0x0194_FD72, 0x79E6_C985, 0x08DD_9701, 0x41CF_CE91];

/// 乱数を 1 つ進める
pub fn ascend(s: [u32; 4]) -> [u32; 4] {
    let [x, y, z, w] = s;
    let t = x ^ (x << 15);
    [y, z, w, w ^ (w >> 21) ^ t ^ (t >> 4)]
}

/// 乱数を 1 つ戻す (ascend の逆)
pub fn descend(s: [u32; 4]) -> [u32; 4] {
    let [x, y, z, w] = s;
    let mut t = w ^ z ^ (z >> 21);
    t ^= t >> 4;
    t ^= t >> 8;
    t ^= t >> 16;
    [t ^ (t << 15) ^ (t << 30), x, y, z]
}

// ---------------------------------------------------------------- GF(2) 多項式

/// 扱う多項式の上限。法が 129 bit、その積が 257 bit なので 320 bit あれば足りる
const LIMBS: usize = 5;
const BITS: usize = LIMBS * 64;

/// GF(2) 上の多項式。ビット i が x^i の係数。リトルエンディアンの u64 配列
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Poly([u64; LIMBS]);

/// 状態遷移行列の特性多項式 f(x) = 0x100000201a8362f671442057eea368001 (129 bit)
const MODULUS: Poly = Poly([0x1442_057e_ea36_8001, 0x0000_0201_a836_2f67, 0x1, 0, 0]);

impl Poly {
    const ZERO: Self = Self([0; LIMBS]);

    const fn from_u64(v: u64) -> Self {
        let mut limbs = [0; LIMBS];
        limbs[0] = v;
        Self(limbs)
    }

    /// 最上位の立っているビットの位置 + 1
    fn bit_len(&self) -> usize {
        for i in (0..LIMBS).rev() {
            if self.0[i] != 0 {
                return i * 64 + 64 - self.0[i].leading_zeros() as usize;
            }
        }
        0
    }

    fn bit(&self, i: usize) -> bool {
        i < BITS && (self.0[i / 64] >> (i % 64)) & 1 == 1
    }

    fn xor(mut self, o: &Self) -> Self {
        for i in 0..LIMBS {
            self.0[i] ^= o.0[i];
        }
        self
    }

    fn shl(&self, n: usize) -> Self {
        debug_assert!(self.bit_len() + n <= BITS, "多項式が {BITS} bit を超えた");
        let (limb, bit) = (n / 64, n % 64);
        let mut out = [0u64; LIMBS];
        for i in (limb..LIMBS).rev() {
            let mut v = self.0[i - limb] << bit;
            if bit > 0 && i > limb {
                v |= self.0[i - limb - 1] >> (64 - bit);
            }
            out[i] = v;
        }
        Self(out)
    }

    /// 繰り上がりなしの掛け算
    fn mul(&self, o: &Self) -> Self {
        let mut acc = Self::ZERO;
        for i in 0..o.bit_len() {
            if o.bit(i) {
                acc = acc.xor(&self.shl(i));
            }
        }
        acc
    }

    /// 剰余
    fn rem(mut self, m: &Self) -> Self {
        let m_len = m.bit_len();
        loop {
            let len = self.bit_len();
            if len < m_len {
                return self;
            }
            self = self.xor(&m.shl(len - m_len));
        }
    }

    /// べき乗の剰余
    fn pow_mod(self, mut exp: u128, m: &Self) -> Self {
        let mut res = Self::from_u64(1);
        let mut base = self.rem(m);
        while exp > 0 {
            if exp & 1 == 1 {
                res = res.mul(&base).rem(m);
            }
            base = base.mul(&base).rem(m);
            exp >>= 1;
        }
        res
    }
}

/// 初期値から frame 回進めた乱数の状態を、多項式を使って一気に求める。
///
/// 指数は本来 `2^128 - 1` で割った余りだが、u64 の範囲では恒等なので省いている
pub fn jump(frame: u64) -> [u32; 4] {
    // r(x) = x^n mod f(x)
    let r = Poly::from_u64(0b10).pow_mod(frame as u128, &MODULUS);
    // v_n = A^n(v_0) = r(A)(v_0)
    let mut s = SEED;
    let mut acc = [0u32; 4];
    for i in 0..r.bit_len() {
        if r.bit(i) {
            for (a, v) in acc.iter_mut().zip(s) {
                *a ^= v;
            }
        }
        s = ascend(s);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// jump(0) は初期値そのもの
    #[test]
    fn jump_zero_is_the_seed() {
        assert_eq!(jump(0), SEED);
    }

    /// jump(n) は ascend を n 回繰り返したものと一致する
    #[test]
    fn jump_matches_repeated_ascend() {
        let mut s = SEED;
        for n in 0..300u64 {
            assert_eq!(jump(n), s, "n = {n}");
            s = ascend(s);
        }
    }

    /// 大きな n でも一致する (足し算で分解できること)
    #[test]
    fn jump_is_additive() {
        let a = jump(1_000_000);
        let mut s = a;
        for _ in 0..500 {
            s = ascend(s);
        }
        assert_eq!(jump(1_000_500), s);
    }

    /// descend は ascend の逆
    #[test]
    fn descend_undoes_ascend() {
        let mut s = jump(12345);
        for _ in 0..100 {
            assert_eq!(descend(ascend(s)), s);
            s = ascend(s);
        }
    }

    #[test]
    fn poly_mul_is_carryless() {
        // (x + 1)^2 = x^2 + 1 (GF(2) では交差項が消える)
        let p = Poly::from_u64(0b11);
        assert_eq!(p.mul(&p), Poly::from_u64(0b101));
    }

    #[test]
    fn poly_rem_reduces_below_the_modulus() {
        let big = Poly::from_u64(0b10).pow_mod(500, &MODULUS);
        assert!(big.bit_len() < MODULUS.bit_len());
    }
}
