//! book のランダム手選択に使う小さな疑似乱数生成器。
//!
//! Egaroucid は独自の xorshift を使うが、選ばれる手の分布が一致する必要は
//! 無いため、ここでは依存を増やさない範囲で xorshift64* を実装する。

#[derive(Clone, Debug)]
pub(crate) struct Random {
    state: u64,
}

impl Random {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            // state == 0 だと以後 0 が出続けるため避ける。
            state: if seed == 0 {
                0x9e37_79b9_7f4a_7c15
            } else {
                seed
            },
        }
    }

    /// 現在時刻から種を作る。時刻が取れない場合は固定値を使う。
    pub(crate) fn from_clock() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x2545_f491_4f6c_dd1d);
        Self::new(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// `[0, 1)` の一様乱数。Egaroucid の `myrandom()`。
    pub(crate) fn next_f64(&mut self) -> f64 {
        // 上位 53 bit を使って [0, 1) に落とす。
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_f64_stays_in_unit_interval() {
        let mut random = Random::new(12345);
        for _ in 0..1000 {
            let v = random.next_f64();
            assert!((0.0..1.0).contains(&v), "{v}");
        }
    }

    #[test]
    fn zero_seed_still_produces_varying_values() {
        let mut random = Random::new(0);
        let first = random.next_u64();
        assert_ne!(first, random.next_u64());
    }
}
