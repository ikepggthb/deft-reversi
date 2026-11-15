use super::*;

/// The plan of a level (1..=60): the midgame to the level's depth at 83%,
/// and the endgame stages of `endgame_limits`.
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
/// The limits of the levels 11..=20, measured in one thread: a stage is used
/// where its time for each disc it saves is at most the time for each disc
/// that the next level gains.
#[rustfmt::skip]
const ENDGAME_LIMITS_11_20: [[i32; 5]; 10] = [
    [16, 16, 17, 17, 19], [16, 16, 18, 18, 20], [16, 16, 18, 19, 21], [17, 17, 19, 20, 22], [17, 17, 19, 21, 22], // 11..=15
    [18, 18, 20, 22, 23], [18, 18, 20, 22, 23], [19, 19, 20, 22, 23], [19, 19, 23, 23, 24], [19, 19, 24, 24, 25], // 16..=20
];
/// The most empties with which a level (1..=60) searches each endgame stage:
/// 100% (exact), 99%, 98%, 95%, 85%. A stage with the limit of the one before
/// it is not used; with more empties than the last limit the level searches
/// the midgame to its own depth.
///
/// - 1..=10: exact up to level + 6, 85% up to level + 9 (the best of the
///   forms played against each other over 1,000 games a level).
/// - 11..=20: the measured table.
/// - 21..=30: exact up to level - 1, then 99%, 98%, 95%, 85% up to level
///   plus 2, 3, 4, 6. From here on a wrong move mostly loses the game, and the
///   endgame search finds the best move more often than a deeper midgame.
/// - 31..=50: one empty more every two levels (the selective stages on the
///   odd level, the exact search on the even one).
/// - 51..=60: two empties more every level, up to the exact search of the
///   whole game at level 60.
///
/// The measurements and matches behind the table are in
/// docs/benchmarks/2026-11-15-level-table.
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
/// What `Goal::Level(level)` reads from a position with `empties` empty
/// squares, as its final iteration would report it; for showing a level's
/// plan without searching. `level` must be 1..=60.
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
    /// The midgame depth, if the level searches the midgame there, and the
    /// selectivity in percent (100 is exact).
    fn config(empties: i32, level: i32) -> (Option<i32>, i32) {
        let (mode, selectivity) = plan(level as u8).at(empties.min(61) as u8);
        let percent = i32::from(selectivity.percent());
        match mode {
            SearchMode::Depth(depth) => (Some(i32::from(depth)), percent),
            _ => (None, percent),
        }
    }
    /// The endgame selectivity of a level in percent, or 0 for its midgame
    /// search.
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
