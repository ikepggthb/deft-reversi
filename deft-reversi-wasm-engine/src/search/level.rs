use super::*;
enum Config {
    Eval(i32, i32),
    Final(i32),
}

/// The reference engine's mapping from a level (1..=60) to the search mode
/// and selectivity, with its opening correction.
pub(super) fn level_plan(empties: u8, level: u8) -> (SearchMode, Selectivity) {
    match level_to_config(i32::from(empties), i32::from(level)) {
        Config::Eval(mut depth, mut selectivity) => {
            if 60 - i32::from(empties) < 20 && depth > 14 {
                depth -= 4;
                if selectivity != 6 {
                    selectivity = 3;
                }
            }
            (
                SearchMode::Depth(depth as u8),
                Selectivity::from_level(selectivity as u8),
            )
        }
        Config::Final(selectivity) => (
            if selectivity == 6 {
                SearchMode::Exact
            } else {
                SearchMode::SelectiveEndgame
            },
            Selectivity::from_level(selectivity as u8),
        ),
    }
}
fn level_to_config(n_empties: i32, level: i32) -> Config {
    // レベルが上がるほど、より空きマスの多い局面から終盤探索へ移る。
    // Final の引数が小さい段階は、MPC を強く使う選択的な終盤探索である。
    use Config::*;
    if level == 0 {
        Eval(0, 6)
    } else if level <= 10 {
        if n_empties <= 2 * level {
            Final(6)
        } else {
            Eval(level, 6)
        }
    } else if level <= 12 {
        if n_empties <= 21 {
            Final(6)
        } else if n_empties <= 24 {
            Final(4)
        } else {
            Eval(level, 2)
        }
    } else if level <= 18 {
        if n_empties <= 21 {
            Final(6)
        } else if n_empties <= 24 {
            Final(4)
        } else if n_empties <= 27 {
            Final(2)
        } else {
            Eval(level, 2)
        }
    } else if level <= 21 {
        if n_empties <= 24 {
            Final(6)
        } else if n_empties <= 27 {
            Final(4)
        } else if n_empties <= 30 {
            Final(2)
        } else {
            Eval(level, 2)
        }
    } else if level <= 24 {
        if n_empties <= 24 {
            Final(6)
        } else if n_empties <= 27 {
            Final(5)
        } else if n_empties <= 30 {
            Final(3)
        } else if n_empties <= 33 {
            Final(1)
        } else {
            Eval(level, 2)
        }
    } else if level <= 27 {
        if n_empties <= 27 {
            Final(6)
        } else if n_empties <= 30 {
            Final(4)
        } else if n_empties <= 33 {
            Final(2)
        } else {
            Eval(level, 2)
        }
    } else if level < 30 {
        if n_empties <= 27 {
            Final(6)
        } else if n_empties <= 30 {
            Final(5)
        } else if n_empties <= 33 {
            Final(3)
        } else if n_empties <= 36 {
            Final(1)
        } else {
            Eval(level, 2)
        }
    } else if level <= 31 {
        if n_empties <= 30 {
            Final(6)
        } else if n_empties <= 33 {
            Final(4)
        } else if n_empties <= 36 {
            Final(2)
        } else {
            Eval(level, 2)
        }
    } else if level <= 33 {
        if n_empties <= 30 {
            Final(6)
        } else if n_empties <= 33 {
            Final(5)
        } else if n_empties <= 36 {
            Final(3)
        } else if n_empties <= 39 {
            Final(1)
        } else {
            Eval(level, 2)
        }
    } else if level <= 35 {
        if n_empties <= 30 {
            Final(6)
        } else if n_empties <= 33 {
            Final(5)
        } else if n_empties <= 36 {
            Final(4)
        } else if n_empties <= 39 {
            Final(2)
        } else {
            Eval(level, 2)
        }
    } else if level < 60 {
        if n_empties <= level - 6 {
            Final(6)
        } else if n_empties <= level - 3 {
            Final(5)
        } else if n_empties <= level {
            Final(4)
        } else if n_empties <= level + 3 {
            Final(3)
        } else if n_empties <= level + 6 {
            Final(2)
        } else if n_empties <= level + 9 {
            Final(1)
        } else {
            Eval(level, 2)
        }
    } else {
        Final(6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_level_boundaries_and_opening_correction() {
        for (empties, level, expected_depth, expected_selectivity) in [
            (60, 8, Some(8), 6),
            (20, 10, None, 6),
            (24, 12, None, 4),
            (27, 18, None, 2),
            (30, 24, None, 3),
            (33, 24, None, 1),
            (27, 28, None, 6),
            (30, 28, None, 5),
            (39, 35, None, 2),
            (30, 36, None, 6),
            (33, 36, None, 5),
            (36, 36, None, 4),
            (39, 36, None, 3),
            (42, 36, None, 2),
            (45, 36, None, 1),
            (46, 36, Some(36), 2),
            (60, 60, None, 6),
        ] {
            match level_to_config(empties, level) {
                Config::Eval(depth, selectivity) => {
                    assert_eq!(Some(depth), expected_depth);
                    assert_eq!(selectivity, expected_selectivity);
                }
                Config::Final(selectivity) => {
                    assert_eq!(expected_depth, None);
                    assert_eq!(selectivity, expected_selectivity);
                }
            }
        }
        assert_eq!(
            level_plan(60, 16),
            (SearchMode::Depth(12), Selectivity::Percent95)
        );
        assert_eq!(level_plan(60, 60).0, SearchMode::Exact);
    }
}
