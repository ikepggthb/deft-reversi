#[cfg(target_feature = "avx2")]
pub use avx2::get_moves;
#[cfg(all(
    not(target_feature = "avx2"),
    not(all(target_arch = "wasm32", target_feature = "simd128"))
))]
pub use generic::get_moves;
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
pub use wasm_simd::get_moves;

// Using AVX2
// Optimization ideas :
//    edax-reversi (Richard Delorme, Toshihiko Okuhara)
//    Edax src               : https://github.com/abulmo/edax-reversi
//    Explanation by Okuhara : http://www.amy.hi-ho.ne.jp/okuhara/bitboard.htm#mobility
#[cfg(target_feature = "avx2")]
mod avx2 {
    use std::arch::x86_64::*;

    #[inline(always)]
    pub fn get_moves(p: u64, o: u64) -> u64 {
        unsafe {
            let shift1897 = _mm256_set_epi64x(7, 9, 8, 1);
            let mfliph = _mm256_set_epi64x(
                0x7e7e7e7e7e7e7e7e,
                0x7e7e7e7e7e7e7e7e,
                -1,
                0x7e7e7e7e7e7e7e7e,
            );

            let pp = _mm256_broadcastq_epi64(_mm_cvtsi64_si128(p as i64));
            let moo =
                _mm256_and_si256(_mm256_broadcastq_epi64(_mm_cvtsi64_si128(o as i64)), mfliph);

            let mut flip_l = _mm256_and_si256(moo, _mm256_sllv_epi64(pp, shift1897));
            let mut flip_r = _mm256_and_si256(moo, _mm256_srlv_epi64(pp, shift1897));

            flip_l = _mm256_or_si256(
                flip_l,
                _mm256_and_si256(moo, _mm256_sllv_epi64(flip_l, shift1897)),
            );
            flip_r = _mm256_or_si256(
                flip_r,
                _mm256_and_si256(moo, _mm256_srlv_epi64(flip_r, shift1897)),
            );

            let pre_l = _mm256_and_si256(moo, _mm256_sllv_epi64(moo, shift1897));
            let pre_r = _mm256_srlv_epi64(pre_l, shift1897);

            let shift2 = _mm256_add_epi64(shift1897, shift1897);

            flip_l = _mm256_or_si256(
                flip_l,
                _mm256_and_si256(pre_l, _mm256_sllv_epi64(flip_l, shift2)),
            );
            flip_r = _mm256_or_si256(
                flip_r,
                _mm256_and_si256(pre_r, _mm256_srlv_epi64(flip_r, shift2)),
            );

            flip_l = _mm256_or_si256(
                flip_l,
                _mm256_and_si256(pre_l, _mm256_sllv_epi64(flip_l, shift2)),
            );
            flip_r = _mm256_or_si256(
                flip_r,
                _mm256_and_si256(pre_r, _mm256_srlv_epi64(flip_r, shift2)),
            );

            let mut mm = _mm256_sllv_epi64(flip_l, shift1897);
            mm = _mm256_or_si256(mm, _mm256_srlv_epi64(flip_r, shift1897));

            let m = _mm_or_si128(_mm256_castsi256_si128(mm), _mm256_extracti128_si256(mm, 1));

            let m = _mm_or_si128(m, _mm_unpackhi_epi64(m, m));

            (_mm_cvtsi128_si64(m) as u64) & !(p | o)
        }
    }
}

#[cfg(all(
    not(target_feature = "avx2"),
    any(test, not(all(target_arch = "wasm32", target_feature = "simd128")))
))]
mod generic {
    #[inline(always)]
    pub fn get_moves(p: u64, o: u64) -> u64 {
        let mut moves: u64;

        let mut flip1: u64;
        let mut flip7: u64;
        let mut flip9: u64;
        let mut flip8: u64;

        let mut pre7: u64;
        let mut pre9: u64;
        let mut pre8: u64;

        // 水平方向マスク処理用(7,9,1方向)のo
        let m_o: u64 = o & 0x7e7e7e7e7e7e7e7e_u64;

        // 正方向（左上7、左下9、下8、右1）
        flip7 = m_o & (p << 7);
        flip9 = m_o & (p << 9);
        flip8 = o & (p << 8);
        flip1 = m_o & (p << 1);

        flip7 |= m_o & (flip7 << 7);
        flip9 |= m_o & (flip9 << 9);
        flip8 |= o & (flip8 << 8);
        moves = m_o + flip1;

        pre7 = m_o & (m_o << 7);
        pre9 = m_o & (m_o << 9);
        pre8 = o & (o << 8);

        flip7 |= pre7 & (flip7 << 14);
        flip9 |= pre9 & (flip9 << 18);
        flip8 |= pre8 & (flip8 << 16);

        flip7 |= pre7 & (flip7 << 14);
        flip9 |= pre9 & (flip9 << 18);
        flip8 |= pre8 & (flip8 << 16);

        moves |= flip7 << 7;
        moves |= flip9 << 9;
        moves |= flip8 << 8;

        // 逆方向（右下7、右上9、上8、左1）
        flip7 = m_o & (p >> 7);
        flip9 = m_o & (p >> 9);
        flip8 = o & (p >> 8);
        flip1 = m_o & (p >> 1);

        flip7 |= m_o & (flip7 >> 7);
        flip9 |= m_o & (flip9 >> 9);
        flip8 |= o & (flip8 >> 8);
        flip1 |= m_o & (flip1 >> 1);

        pre7 >>= 7;
        pre9 >>= 9;
        pre8 >>= 8;
        let pre1: u64 = m_o & (m_o >> 1);

        flip7 |= pre7 & (flip7 >> 14);
        flip9 |= pre9 & (flip9 >> 18);
        flip8 |= pre8 & (flip8 >> 16);
        flip1 |= pre1 & (flip1 >> 2);

        flip7 |= pre7 & (flip7 >> 14);
        flip9 |= pre9 & (flip9 >> 18);
        flip8 |= pre8 & (flip8 >> 16);
        flip1 |= pre1 & (flip1 >> 2);

        moves |= flip7 >> 7;
        moves |= flip9 >> 9;
        moves |= flip8 >> 8;
        moves |= flip1 >> 1;

        // 空きマスでマスク
        moves & !(p | o)
    }
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod wasm_simd {
    use std::arch::wasm32::*;
    // Reverse bits in each 64-bit lane via a nibble lookup and byte shuffle.
    // WASM SIMD has uniform shifts, so reversal lets the two lanes process
    // opposite directions with the same shift count.
    #[inline(always)]
    fn reverse(v: v128) -> v128 {
        let table = u8x16(0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15);
        let lo = v128_and(v, u8x16_splat(15));
        let hi = v128_and(u8x16_shr(v, 4), u8x16_splat(15));
        let bits = v128_or(
            i8x16_shl(i8x16_swizzle(table, lo), 4),
            i8x16_swizzle(table, hi),
        );
        i8x16_shuffle::<7, 6, 5, 4, 3, 2, 1, 0, 15, 14, 13, 12, 11, 10, 9, 8>(bits, bits)
    }
    #[inline(always)]
    pub fn get_moves(p: u64, o: u64) -> u64 {
        // Reversal turns all right shifts into left shifts; each SIMD lane
        // computes one direction. Horizontal propagation retains the carry trick.
        let pp0 = u64x2_splat(p);
        let oo0 = u64x2_splat(o);
        let pp = i64x2_shuffle::<0, 2>(pp0, reverse(pp0));
        let oo = i64x2_shuffle::<0, 2>(oo0, reverse(oo0));
        let mm = v128_and(oo, u64x2_splat(0x7e7e7e7e7e7e7e7e));
        let one = v128_and(mm, i64x2_shl(pp, 1));
        let mut result = i64x2_add(mm, one);
        macro_rules! ray {
            ($opp:expr, $shift:expr) => {{
                let m = $opp;
                let mut f = v128_and(m, i64x2_shl(pp, $shift));
                f = v128_or(f, v128_and(m, i64x2_shl(f, $shift)));
                let pre = v128_and(m, i64x2_shl(m, $shift));
                f = v128_or(f, v128_and(pre, i64x2_shl(f, $shift * 2)));
                f = v128_or(f, v128_and(pre, i64x2_shl(f, $shift * 2)));
                i64x2_shl(f, $shift)
            }};
        }
        result = v128_or(result, ray!(mm, 7));
        result = v128_or(result, ray!(oo, 8));
        result = v128_or(result, ray!(mm, 9));
        let left = u64x2_extract_lane::<0>(result);
        let right = u64x2_extract_lane::<1>(reverse(result));
        (left | right) & !(p | o)
    }
}

#[cfg(all(test, target_arch = "wasm32", target_feature = "simd128"))]
mod wasm_simd_tests {
    use wasm_bindgen_test::wasm_bindgen_test;
    #[wasm_bindgen_test]
    fn moves_match_generic_for_disjoint_boards() {
        let mut seed = 0xfedc_ba98_7654_3210u64;
        for _ in 0..100_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let player = seed;
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let opponent = seed & !player;
            assert_eq!(
                super::wasm_simd::get_moves(player, opponent),
                super::generic::get_moves(player, opponent)
            );
        }
    }
}
