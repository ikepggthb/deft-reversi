//! book が 1 局面について持つ値。
//!
//! Edax は評価値と上下界を、Egaroucid は評価値と探索レベルだけを持つ。
//! どちらも「その値がどれだけ信用できるか」を後から復元できない。
//!
//! - Edax の level は Edax の探索表に、Egaroucid の level は Egaroucid の
//!   探索表に紐づいていて、エンジンをまたぐと意味が変わる
//! - 上下界が無いと「確定値なのか、まだ動きうるのか」が分からない
//!
//! ここでは **探索の中身そのもの** (深さ・選択度・完全読みか) を持たせる。
//! これがあると
//!
//! - どこを掘る価値が高いかを不確かさから決められる ([`BookValue::uncertainty`])
//! - 対局中に「book の値が怪しいので探索し直す」判断ができる
//! - レベルを上げて育て直すとき、何を再探索すべきかが分かる
//!
//! ようになる。

/// 石差スコアの絶対値の上限。
pub const SCORE_MAX: i8 = 64;
/// 値が入っていないことを表すスコア。
pub const SCORE_UNDEFINED: i8 = -128;
/// 探索深さが不明。
pub const DEPTH_UNKNOWN: u8 = 255;

/// MPC を使わない (完全な) 選択度。
pub const SELECTIVITY_EXACT: u8 = 6;

/// 手が無いことを表す疑似座標。
pub const MOVE_NONE: i8 = -1;
/// パスを表す疑似座標。
pub const MOVE_PASS: i8 = 64;

/// 探索スコアを book の 1 バイトスコアに丸める。
pub fn clamp_score(score: i32) -> i8 {
    score.clamp(-(SCORE_MAX as i32), SCORE_MAX as i32) as i8
}

/// 盤上の座標として有効か。
pub fn is_valid_move(mv: i8) -> bool {
    (0..64).contains(&mv)
}

/// 石差として有効か。
pub fn is_valid_score(score: i8) -> bool {
    (-SCORE_MAX..=SCORE_MAX).contains(&score)
}

/// 探索結果に見込む誤差 (石差)。
///
/// book の値に窓を持たせるための見積もり。終局まで読み切って選択度も完全なら
/// 0、そうでなければ「残りの空きマスをどれだけ読み残したか」と選択度から
/// 大きめに見積もる。育成はこの窓が広いところを優先して掘るので、
/// 見積もりが多少粗くても「掘る順番」としては機能する。
///
/// 中盤探索の実測誤差は深さ 10 前後で 3〜4 石、深さ 20 で 2 石程度なので、
/// その水準に合わせてある。
pub fn search_error(n_empties: u8, depth: u8, selectivity: u8) -> i8 {
    if depth == DEPTH_UNKNOWN {
        return SCORE_MAX;
    }
    // 完全読み。
    if depth >= n_empties && selectivity >= SELECTIVITY_EXACT {
        return 0;
    }
    // 終局まで読んだが枝刈りしている。選択度が低いほど広い。
    if depth >= n_empties {
        return (SELECTIVITY_EXACT.saturating_sub(selectivity) as i8).min(6);
    }
    // 中盤探索。深さが浅いほど広い。
    let base = 8i32 - (depth as i32) / 4;
    let selective = (SELECTIVITY_EXACT.saturating_sub(selectivity) as i32) / 2;
    (base.clamp(2, 12) + selective).min(SCORE_MAX as i32) as i8
}

/// 値の素性。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ValueFlags(u8);

impl ValueFlags {
    /// 終局まで読み切った確定値。
    pub const EXACT: Self = Self(0b0000_0001);
    /// 終盤探索 (石差が出るところまで読んだ) の値。
    pub const ENDGAME: Self = Self(0b0000_0010);
    /// 人手で設定した値。育成で上書きしない。
    pub const PINNED: Self = Self(0b0000_0100);
    /// 子局面から伝播した値 (自分自身を探索したわけではない)。
    pub const PROPAGATED: Self = Self(0b0000_1000);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// book が 1 局面について持つ値。8 バイト。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookValue {
    /// 手番側から見た評価値 (石差)。
    pub score: i8,
    /// 下界。確定値なら `score` と等しい。
    pub lower: i8,
    /// 上界。確定値なら `score` と等しい。
    pub upper: i8,
    /// 探索した深さ (手数)。空きマス数と等しければ終局まで読んでいる。
    pub depth: u8,
    /// MPC の選択度。[`SELECTIVITY_EXACT`] なら枝刈りなし。
    pub selectivity: u8,
    pub flags: ValueFlags,
}

impl Default for BookValue {
    fn default() -> Self {
        Self::undefined()
    }
}

impl BookValue {
    /// 値が入っていない状態。
    pub const fn undefined() -> Self {
        Self {
            score: SCORE_UNDEFINED,
            lower: -SCORE_MAX,
            upper: SCORE_MAX,
            depth: DEPTH_UNKNOWN,
            selectivity: 0,
            flags: ValueFlags::empty(),
        }
    }

    /// 終局まで読み切った確定値。
    pub const fn exact(score: i8) -> Self {
        Self {
            score,
            lower: score,
            upper: score,
            depth: 60,
            selectivity: SELECTIVITY_EXACT,
            flags: ValueFlags::from_bits(ValueFlags::EXACT.bits() | ValueFlags::ENDGAME.bits()),
        }
    }

    /// 探索結果から作る。
    ///
    /// `n_empties` と `depth` が等しく選択度が完全なら確定値として扱い、
    /// 上下界を `score` に潰す。そうでなければ `error` を見込んだ幅を持たせる。
    pub fn from_search(score: i8, n_empties: u8, depth: u8, selectivity: u8, error: i8) -> Self {
        let reaches_end = depth >= n_empties;
        let exact = reaches_end && selectivity >= SELECTIVITY_EXACT;
        let mut flags = ValueFlags::empty();
        if exact {
            flags = flags.union(ValueFlags::EXACT);
        }
        if reaches_end {
            flags = flags.union(ValueFlags::ENDGAME);
        }

        let (lower, upper) = if exact {
            (score, score)
        } else {
            (
                score.saturating_sub(error).max(-SCORE_MAX),
                score.saturating_add(error).min(SCORE_MAX),
            )
        };
        Self {
            score,
            lower,
            upper,
            depth,
            selectivity,
            flags,
        }
    }

    /// 探索結果から作る。誤差は [`search_error`] で見積もる。
    pub fn searched(score: i8, n_empties: u8, depth: u8, selectivity: u8) -> Self {
        let error = search_error(n_empties, depth, selectivity);
        Self::from_search(score, n_empties, depth, selectivity, error)
    }

    /// 上下界を明示して作る。子から伝播させた値を組み立てるのに使う。
    pub fn with_bounds(score: i8, lower: i8, upper: i8, depth: u8, selectivity: u8) -> Self {
        let lower = lower.max(-SCORE_MAX);
        let upper = upper.min(SCORE_MAX);
        let mut flags = ValueFlags::empty();
        if lower == upper {
            flags = flags.union(ValueFlags::EXACT).union(ValueFlags::ENDGAME);
        }
        Self {
            score: score.clamp(lower, upper),
            lower,
            upper,
            depth,
            selectivity,
            flags,
        }
    }

    /// 値が入っているか。
    pub fn is_defined(&self) -> bool {
        self.score != SCORE_UNDEFINED
    }

    /// 終局まで読み切った確定値か。
    pub fn is_exact(&self) -> bool {
        self.flags.contains(ValueFlags::EXACT)
    }

    /// 人手で固定された値か。
    pub fn is_pinned(&self) -> bool {
        self.flags.contains(ValueFlags::PINNED)
    }

    /// 値の不確かさ (上界 - 下界)。確定値なら 0。
    ///
    /// 育成でどこを掘るかを決めるときの主要な材料。
    pub fn uncertainty(&self) -> i32 {
        if !self.is_defined() {
            // 未探索は最も不確か。
            return 2 * SCORE_MAX as i32;
        }
        self.upper as i32 - self.lower as i32
    }

    /// 手番を入れ替えた値。親から子を見るときに使う。
    pub fn negated(&self) -> Self {
        if !self.is_defined() {
            return *self;
        }
        Self {
            score: -self.score,
            lower: -self.upper,
            upper: -self.lower,
            ..*self
        }
    }

    /// `other` の方が信用できるなら `true`。
    ///
    /// 確定値 > 終盤 > 深い探索 > 選択度が高い、の順に見る。
    pub fn is_superseded_by(&self, other: &Self) -> bool {
        if !other.is_defined() {
            return false;
        }
        if !self.is_defined() {
            return true;
        }
        if self.is_pinned() {
            return false;
        }
        if other.is_exact() != self.is_exact() {
            return other.is_exact();
        }
        (other.depth, other.selectivity) > (self.depth, self.selectivity)
    }
}

/// まだ book に無い手のうち最善のもの。育成の起点になる。
///
/// Egaroucid の `Leaf` に相当するが、探索の深さも持たせて、掘る価値の判定に
/// 使えるようにしてある。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frontier {
    /// 手。[`MOVE_NONE`] なら「もう展開できる手が無い」。
    pub mv: i8,
    /// その手の評価値 (親の手番から見た石差)。
    pub score: i8,
    /// 評価に使った探索深さ。
    pub depth: u8,
}

impl Default for Frontier {
    fn default() -> Self {
        Self::none()
    }
}

impl Frontier {
    /// 未設定。
    pub const fn unset() -> Self {
        Self {
            mv: MOVE_NONE,
            score: SCORE_UNDEFINED,
            depth: DEPTH_UNKNOWN,
        }
    }

    /// 展開できる手が無い。
    pub const fn none() -> Self {
        Self {
            mv: MOVE_NONE,
            score: SCORE_UNDEFINED,
            depth: 0,
        }
    }

    /// 実際に指せる手を持っているか。
    pub fn has_move(&self) -> bool {
        (0..64).contains(&self.mv) && self.score != SCORE_UNDEFINED
    }

    /// 「もう展開できる手が無い」と分かっている状態か。
    pub fn is_exhausted(&self) -> bool {
        self.mv == MOVE_NONE && self.depth != DEPTH_UNKNOWN
    }

    /// まだ探索していない状態か。
    pub fn is_unset(&self) -> bool {
        self.mv == MOVE_NONE && self.depth == DEPTH_UNKNOWN
    }

    /// この手が親にもたらしうる値の上限の見積もり。
    ///
    /// 「book に入っていない手の中の最善」なので、親の値の上界を抑えるのに
    /// 使える。まだ調べていなければ `None` (上界は分からない)。
    pub fn upper_estimate(&self, n_empties: u8) -> Option<i8> {
        if self.is_exhausted() {
            // 展開できる手が無い = 上界に寄与しない。
            return Some(-SCORE_MAX);
        }
        if !self.has_move() {
            return None;
        }
        let error = search_error(n_empties, self.depth, SELECTIVITY_EXACT);
        Some(self.score.saturating_add(error).min(SCORE_MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_search_marks_a_full_depth_exact_search() {
        let value = BookValue::from_search(4, 20, 20, SELECTIVITY_EXACT, 2);
        assert!(value.is_exact());
        assert_eq!((value.lower, value.upper), (4, 4));
        assert_eq!(value.uncertainty(), 0);
    }

    #[test]
    fn from_search_keeps_a_window_for_a_selective_search() {
        let value = BookValue::from_search(4, 20, 20, 3, 2);
        assert!(!value.is_exact(), "選択的な探索は確定値ではない");
        assert!(value.flags.contains(ValueFlags::ENDGAME));
        assert_eq!((value.lower, value.upper), (2, 6));
        assert_eq!(value.uncertainty(), 4);
    }

    #[test]
    fn from_search_marks_a_midgame_search() {
        let value = BookValue::from_search(4, 40, 12, 3, 3);
        assert!(!value.is_exact());
        assert!(!value.flags.contains(ValueFlags::ENDGAME));
        assert_eq!((value.lower, value.upper), (1, 7));
    }

    #[test]
    fn window_is_clamped_to_the_score_range() {
        let value = BookValue::from_search(63, 40, 12, 3, 5);
        assert_eq!(value.upper, SCORE_MAX);
        let value = BookValue::from_search(-63, 40, 12, 3, 5);
        assert_eq!(value.lower, -SCORE_MAX);
    }

    #[test]
    fn undefined_is_the_most_uncertain() {
        let value = BookValue::undefined();
        assert!(!value.is_defined());
        assert_eq!(value.uncertainty(), 128);
    }

    #[test]
    fn negated_swaps_and_flips_the_bounds() {
        let value = BookValue::from_search(4, 40, 12, 3, 2);
        let negated = value.negated();
        assert_eq!(negated.score, -4);
        assert_eq!(negated.lower, -value.upper);
        assert_eq!(negated.upper, -value.lower);
        // 2 回で戻る。
        assert_eq!(negated.negated(), value);
    }

    #[test]
    fn superseded_prefers_exact_then_depth_then_selectivity() {
        let midgame = BookValue::from_search(0, 40, 12, 3, 2);
        let deeper = BookValue::from_search(0, 40, 16, 3, 2);
        let selective = BookValue::from_search(0, 40, 12, 5, 2);
        let exact = BookValue::from_search(0, 20, 20, SELECTIVITY_EXACT, 2);

        assert!(midgame.is_superseded_by(&deeper));
        assert!(midgame.is_superseded_by(&selective));
        assert!(midgame.is_superseded_by(&exact));
        assert!(!deeper.is_superseded_by(&midgame));
        assert!(!exact.is_superseded_by(&deeper), "確定値は深さで負けない");
    }

    #[test]
    fn pinned_values_are_never_superseded() {
        let mut pinned = BookValue::from_search(0, 40, 12, 3, 2);
        pinned.flags = pinned.flags.union(ValueFlags::PINNED);
        let exact = BookValue::from_search(0, 20, 20, SELECTIVITY_EXACT, 2);
        assert!(!pinned.is_superseded_by(&exact));
    }

    #[test]
    fn undefined_is_superseded_by_anything_defined() {
        let undefined = BookValue::undefined();
        let any = BookValue::from_search(0, 40, 2, 0, 8);
        assert!(undefined.is_superseded_by(&any));
        assert!(!any.is_superseded_by(&undefined));
    }

    #[test]
    fn frontier_states_are_distinguishable() {
        assert!(Frontier::unset().is_unset());
        assert!(!Frontier::unset().is_exhausted());
        assert!(Frontier::none().is_exhausted());
        assert!(!Frontier::none().has_move());

        let with_move = Frontier {
            mv: 19,
            score: 2,
            depth: 10,
        };
        assert!(with_move.has_move());
        assert!(!with_move.is_unset());
    }
}
