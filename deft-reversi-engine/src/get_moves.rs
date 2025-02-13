
// Using AVX2
// Optimization ideas : 
//    edax-reversi (Richard Delorme, Toshihiko Okuhara)
//    Edax src               : https://github.com/abulmo/edax-reversi
//    Explanation by Okuhara : http://www.amy.hi-ho.ne.jp/okuhara/bitboard.htm#mobility

#[cfg(target_feature = "avx2")]
use std::arch::x86_64::*;
#[cfg(target_feature = "avx2")]
#[inline(always)]
pub fn get_moves(p: u64, o: u64) -> u64 {
    unsafe {
        let shift1897 = _mm256_set_epi64x(7, 9, 8, 1);
        let mfliph = _mm256_set_epi64x(
            0x7e7e7e7e7e7e7e7e, 
            0x7e7e7e7e7e7e7e7e, 
            -1i64 as u64 as i64, 
            0x7e7e7e7e7e7e7e7e
        );
    
        let pp = _mm256_broadcastq_epi64(_mm_cvtsi64_si128(p as i64));
        let moo = _mm256_and_si256(
            _mm256_broadcastq_epi64(_mm_cvtsi64_si128(o as i64)), 
            mfliph
        );
    
        let mut flip_l = _mm256_and_si256(moo, _mm256_sllv_epi64(pp, shift1897));
        let mut flip_r = _mm256_and_si256(moo, _mm256_srlv_epi64(pp, shift1897));
    
        flip_l = _mm256_or_si256(flip_l, _mm256_and_si256(moo, _mm256_sllv_epi64(flip_l, shift1897)));
        flip_r = _mm256_or_si256(flip_r, _mm256_and_si256(moo, _mm256_srlv_epi64(flip_r, shift1897)));
    
        let pre_l = _mm256_and_si256(moo, _mm256_sllv_epi64(moo, shift1897));
        let pre_r = _mm256_srlv_epi64(pre_l, shift1897);
    
        let shift2 = _mm256_add_epi64(shift1897, shift1897);
    
        flip_l = _mm256_or_si256(flip_l, _mm256_and_si256(pre_l, _mm256_sllv_epi64(flip_l, shift2)));
        flip_r = _mm256_or_si256(flip_r, _mm256_and_si256(pre_r, _mm256_srlv_epi64(flip_r, shift2)));
    
        flip_l = _mm256_or_si256(flip_l, _mm256_and_si256(pre_l, _mm256_sllv_epi64(flip_l, shift2)));
        flip_r = _mm256_or_si256(flip_r, _mm256_and_si256(pre_r, _mm256_srlv_epi64(flip_r, shift2)));
    
        let mut mm = _mm256_sllv_epi64(flip_l, shift1897);
        mm = _mm256_or_si256(mm, _mm256_srlv_epi64(flip_r, shift1897));
    
        let m = _mm_or_si128(
            _mm256_castsi256_si128(mm),
            _mm256_extracti128_si256(mm, 1)
        );
    
        let m = _mm_or_si128(m, _mm_unpackhi_epi64(m, m));
    
        (_mm_cvtsi128_si64(m) as u64) & !(p | o)
    }
}


#[cfg(not(target_feature = "avx2"))]
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
    flip7  = m_o & (p << 7);
    flip9  = m_o & (p << 9);
    flip8  = o & (p << 8);
    flip1  = m_o & (p << 1);

    flip7 |= m_o & (flip7 << 7);
    flip9 |= m_o & (flip9 << 9);
    flip8 |= o  & (flip8 << 8);
    moves  = m_o + flip1; 

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
    flip8 |= o  & (flip8 >> 8);
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

