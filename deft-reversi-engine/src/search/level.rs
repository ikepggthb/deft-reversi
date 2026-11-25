//! 強さレベル（`Goal::Level(1..=60)`）に対応する中盤探索深さと終盤選択探索のスケジュールを定義します。
use super::*;

/// レベル（1..=60）に対応する [`SearchPlan`] を構築します。
/// 中盤は 83% 選択率でレベルと同じ深さまで読み、終盤は `endgame_limits` に従って段階的に完全読みへ移行します。
pub(super) fn plan(level: u8) -> SearchPlan {
    let limits = endgame_limits(i32::from(level));
    [
        Selectivity::Full,
        Selectivity::Percent99,
        Selectivity::Percent98,
        Selectivity::Percent95,
        Selectivity::Percent85,
    ]
    .into_iter()
    .zip(limits)
    .fold(
        SearchPlan::midgame(level, Selectivity::Percent83),
        |plan, (stage, limit)| plan.endgame(stage, limit as u8),
    )
}
/// レベル 11..=20 の終盤切り替え空きマス数テーブル（1 スレッド実測ベース）です。
/// 1 石あたりの所要時間が次のレベルで得られる利得を下回る境界で各ステージを採用しています。
#[rustfmt::skip]
const ENDGAME_LIMITS_11_20: [[i32; 5]; 10] = [
    [16, 16, 17, 17, 19], [16, 16, 18, 18, 20], [16, 16, 18, 19, 21], [17, 17, 19, 20, 22], [17, 17, 19, 21, 22], // 11..=15
    [18, 18, 20, 22, 23], [18, 18, 20, 22, 23], [19, 19, 20, 22, 23], [19, 19, 23, 23, 24], [19, 19, 24, 24, 25], // 16..=20
];
/// 各レベル（1..=60）が終盤ステージ（100% 完全読み, 99%, 98%, 95%, 85%）を適用する最大空きマス数です。
///
/// 1 つ前のステージと同じ上限値を持つステージはスキップします。
/// 最後の上限を超える空きマス数では、中盤探索（深さ = レベル、選択率 83%）を行います。
/// - 1..=10: `level + 6` まで完全読み、`level + 9` まで 85%（各レベル 1,000 局の対戦実験で選定しました）。
/// - 11..=20: 実測テーブルを使います。
/// - 21..=30: `level - 1` まで完全読み、以降 `level + 2, 3, 4, 6` まで 99%, 98%, 95%, 85% とします。
///   この領域からは 1 度の悪手が即敗北に繋がるため、深い中盤読みより終盤選択読みを優先します。
/// - 31..=50: 2 レベルごとに空きマス上限が 1 増えます（奇数レベルで選択ステージ、偶数レベルで完全読みが上昇します）。
/// - 51..=60: 1 レベルごとに空きマス上限が 2 増え、レベル 60 で初手（空き 60）から完全読みに到達します。
fn endgame_limits(level: i32) -> [i32; 5] {
    match level {
        ..=10 => [level + 6, level + 6, level + 6, level + 6, level + 9],
        11..=20 => ENDGAME_LIMITS_11_20[(level - 11) as usize],
        _ => {
            let (exact, odd) = match level {
                ..=30 => (level - 1, 0),
                31..=50 => (14 + level / 2, level % 2),
                _ => (2 * level - 60, 0),
            };
            [0, 3 + odd, 4 + odd, 5 + odd, 7 + odd].map(|more| (exact + more).min(60))
        }
    }
}
/// 探索を実行せずに、`Goal::Level(level)` が空きマス数 `empties` の局面に対して
/// 最終イテレーションで到達する予定の探索種別（[`ResultKind`]）を返します。`level` は 1..=60 です。
pub fn level_reading(level: u8, empties: u8) -> ResultKind {
    match plan(level).at(empties) {
        (SearchMode::Depth(depth), selectivity) => ResultKind::Depth {
            depth: depth.min(empties),
            selectivity,
        },
        (SearchMode::Exact | SearchMode::SelectiveEndgame, selectivity) => {
            ResultKind::Endgame { selectivity }
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn level_reading_matches_the_plan() {
        use super::*;
        let depth = |depth, selectivity| ResultKind::Depth { depth, selectivity };
        let endgame = |selectivity| ResultKind::Endgame { selectivity };
        assert_eq!(level_reading(25, 60), depth(25, Selectivity::Percent83));
        assert_eq!(level_reading(25, 32), depth(25, Selectivity::Percent83));
        assert_eq!(level_reading(25, 31), endgame(Selectivity::Percent85));
        assert_eq!(level_reading(25, 24), endgame(Selectivity::Full));
        assert_eq!(level_reading(1, 50), depth(1, Selectivity::Percent83));
        assert_eq!(level_reading(1, 7), endgame(Selectivity::Full));
        assert_eq!(level_reading(60, 60), endgame(Selectivity::Full));
    }
    use super::*;
    /// その空きマス数で中盤探索を行う場合は中盤深さと選択率（%・完全読みは 100）を返します。
    fn config(empties: i32, level: i32) -> (Option<i32>, i32) {
        let (mode, selectivity) = plan(level as u8).at(empties.min(61) as u8);
        let percent = i32::from(selectivity.percent());
        match mode {
            SearchMode::Depth(depth) => (Some(i32::from(depth)), percent),
            _ => (None, percent),
        }
    }
    /// 終盤探索の選択率（%）を返します。中盤探索の場合は 0 を返します。
    fn stage(empties: i32, level: i32) -> i32 {
        match config(empties, level) {
            (Some(depth), percent) => {
                assert_eq!((depth, percent), (level, 83));
                0
            }
            (None, selectivity) => selectivity,
        }
    }
    #[test]
    fn midgame_of_a_level_is_its_depth_at_83() {
        for level in 1..=60 {
            let plan = plan(level);
            assert_eq!(plan.midgame_depth(), level);
            assert_eq!(plan.midgame_selectivity(), Selectivity::Percent83);
        }
        assert_eq!(
            plan(10).at(40),
            (SearchMode::Depth(10), Selectivity::Percent83)
        );
    }
    #[test]
    fn accuracy_never_falls_with_the_level_nor_rises_with_the_empties() {
        for level in 1..=60 {
            for empties in 1..=60 {
                let here = stage(empties, level);
                assert!(
                    here >= stage(empties + 1, level),
                    "level {level} at {empties} empties"
                );
                if level < 60 {
                    assert!(
                        here <= stage(empties, level + 1),
                        "level {level} at {empties} empties"
                    );
                }
            }
        }
    }
    #[test]
    fn level_boundaries() {
        for (empties, level, expected_depth, expected_selectivity) in [
            (60, 8, Some(8), 83),
            (7, 1, None, 100),
            (10, 1, None, 85),
            (11, 1, Some(1), 83),
            (13, 7, None, 100),
            (16, 7, None, 85),
            (17, 7, Some(7), 83),
            (16, 10, None, 100),
            (19, 10, None, 85),
            (20, 10, Some(10), 83),
            (16, 12, None, 100),
            (18, 12, None, 98),
            (20, 12, None, 85),
            (21, 12, Some(12), 83),
            (18, 16, None, 100),
            (20, 16, None, 98),
            (22, 16, None, 95),
            (23, 16, None, 85),
            (24, 16, Some(16), 83),
            (19, 20, None, 100),
            (24, 20, None, 98),
            (25, 20, None, 85),
            (24, 25, None, 100),
            (27, 25, None, 99),
            (28, 25, None, 98),
            (29, 25, None, 95),
            (31, 25, None, 85),
            (32, 25, Some(25), 83),
            (29, 30, None, 100),
            (32, 30, None, 99),
            (33, 30, None, 98),
            (34, 30, None, 95),
            (36, 30, None, 85),
            (37, 30, Some(30), 83),
            (32, 36, None, 100),
            (35, 36, None, 99),
            (39, 36, None, 85),
            (40, 36, Some(36), 83),
            (32, 37, None, 100),
            (36, 37, None, 99),
            (40, 37, None, 85),
            (39, 50, None, 100),
            (46, 50, None, 85),
            (47, 50, Some(50), 83),
            (46, 53, None, 100),
            (53, 53, None, 85),
            (54, 53, Some(53), 83),
            (54, 57, None, 100),
            (59, 57, None, 95),
            (60, 57, None, 85),
            (58, 59, None, 100),
            (60, 59, None, 99),
            (60, 60, None, 100),
        ] {
            let (depth, selectivity) = config(empties, level);
            assert_eq!(depth, expected_depth, "level {level} at {empties} empties");
            assert_eq!(selectivity, expected_selectivity);
        }
        assert_eq!(
            plan(16).at(60),
            (SearchMode::Depth(16), Selectivity::Percent83)
        );
        assert_eq!(plan(60).at(60).0, SearchMode::Exact);
    }
}
