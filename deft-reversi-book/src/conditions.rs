//! 計算した評価に含まれる中盤・終盤・旧評価それぞれの探索条件。
use super::*;
/// 合成した値を使うために確認すべき、元の評価の種類と最低の探索条件。
/// 中盤の深さ・選択率、終盤の選択率、旧データの条件を別々に集計する。
/// 全体が正しい確率ではない。horizonだけから探索の種類を判断してはいけない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SearchConditions {
    pub(crate) families: u8,
    pub(crate) horizon: u8,
    // 条件のない種類は255。includeで最小を取る際に混ざらず、validateで整合を検査する。
    pub(crate) mid_depth: u8,
    pub(crate) mid_sel: u8,
    pub(crate) end_sel: u8,
    pub(crate) legacy_depth: u8,
    pub(crate) legacy_sel: u8,
}
impl SearchConditions {
    pub(crate) const MIDGAME: u8 = 1;
    pub(crate) const ENDGAME: u8 = 2;
    pub(crate) const LEGACY: u8 = 4;
    pub(crate) const RULES: u8 = 8;
    pub(crate) const MANUAL: u8 = 16;
    pub(crate) fn empty() -> Self {
        Self {
            families: 0,
            horizon: 60,
            mid_depth: 255,
            mid_sel: 255,
            end_sel: 255,
            legacy_depth: 255,
            legacy_sel: 255,
        }
    }
    pub(crate) fn legacy(depth: u8, sel: Selectivity) -> Self {
        Self {
            families: Self::LEGACY,
            horizon: depth,
            legacy_depth: depth,
            legacy_sel: sel as u8,
            ..Self::empty()
        }
    }
    pub(crate) fn search(v: CompletedSearch) -> Self {
        match v.completed {
            SearchKind::Midgame { depth, selectivity } => Self {
                families: Self::MIDGAME,
                horizon: depth,
                mid_depth: depth,
                mid_sel: selectivity as u8,
                ..Self::empty()
            },
            SearchKind::Endgame { selectivity } => Self {
                families: Self::ENDGAME,
                horizon: v.board().expect("validated search root").empty_count(),
                end_sel: selectivity as u8,
                ..Self::empty()
            },
        }
    }
    pub fn has_midgame(self) -> bool {
        self.families & Self::MIDGAME != 0
    }
    pub fn has_endgame(self) -> bool {
        self.families & Self::ENDGAME != 0
    }
    pub fn has_manual(self) -> bool {
        self.families & Self::MANUAL != 0
    }
    pub fn imported_depth(self) -> Option<u8> {
        self.has_imported().then_some(self.legacy_depth)
    }
    pub fn imported_selectivity(self) -> Option<Selectivity> {
        self.has_imported()
            .then(|| selectivity(self.legacy_sel).unwrap())
    }
    pub fn has_imported(self) -> bool {
        self.families & Self::LEGACY != 0
    }
    pub fn midgame_depth(self) -> Option<u8> {
        (self.mid_depth != 255).then_some(self.mid_depth)
    }
    pub fn midgame_selectivity(self) -> Option<Selectivity> {
        (self.mid_sel != 255).then(|| selectivity(self.mid_sel).unwrap())
    }
    pub fn endgame_selectivity(self) -> Option<Selectivity> {
        (self.end_sel != 255).then(|| selectivity(self.end_sel).unwrap())
    }
    pub fn depth(self) -> u8 {
        self.horizon
    }
    pub(crate) fn minimum_selectivity(self) -> Selectivity {
        selectivity(
            [self.mid_sel, self.end_sel, self.legacy_sel]
                .into_iter()
                .min()
                .unwrap()
                .min(6),
        )
        .unwrap()
    }
    // 子から親へは点数の符号だけでなく到達深さも変わる。石を置いた場合だけ+1。
    // パスは手番を変えるが深さを消費しない。中盤・終盤という種類も変えない。
    pub(crate) fn shifted(mut self, placement: bool) -> Self {
        let d = u8::from(placement);
        self.horizon = (self.horizon + d).min(60);
        if self.mid_depth != 255 {
            self.mid_depth = (self.mid_depth + d).min(60)
        }
        if self.legacy_depth != 255 {
            self.legacy_depth = (self.legacy_depth + d).min(60)
        }
        self
    }
    pub(crate) fn include(&mut self, v: Self) {
        // Manual input has no search-depth requirement. Its display fields must
        // not lower the recorded quality of real legacy searches in a mixture.
        let had_legacy = self.has_imported();
        self.families |= v.families;
        self.horizon = self.horizon.min(v.horizon);
        self.mid_depth = self.mid_depth.min(v.mid_depth);
        self.mid_sel = self.mid_sel.min(v.mid_sel);
        self.end_sel = self.end_sel.min(v.end_sel);
        if v.has_imported() {
            self.legacy_depth = if had_legacy {
                self.legacy_depth.min(v.legacy_depth)
            } else {
                v.legacy_depth
            };
            self.legacy_sel = if had_legacy {
                self.legacy_sel.min(v.legacy_sel)
            } else {
                v.legacy_sel
            };
        } else if !had_legacy {
            self.legacy_depth = self.legacy_depth.min(v.legacy_depth);
            self.legacy_sel = self.legacy_sel.min(v.legacy_sel);
        }
    }
    pub(crate) fn validate(self) -> Result<()> {
        if self.families == 0
            || self.families & !31 != 0
            || self.horizon > 60
            || [self.mid_depth, self.legacy_depth]
                .into_iter()
                .any(|v| v != 255 && v > 60)
            || [self.mid_sel, self.end_sel, self.legacy_sel]
                .into_iter()
                .any(|v| v != 255 && v > 6)
            || self.has_midgame() != (self.mid_depth != 255)
            || self.has_midgame() != (self.mid_sel != 255)
            || self.has_endgame() != (self.end_sel != 255)
            || (self.families & (Self::LEGACY | Self::MANUAL) != 0) != (self.legacy_depth != 255)
            || (self.families & (Self::LEGACY | Self::MANUAL) != 0) != (self.legacy_sel != 255)
        {
            return Err(invalid("quality summary"));
        }
        Ok(())
    }
}
