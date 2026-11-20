//! Structured clock log: with `DEFT_CLOCK_LOG=FILE`, one JSON object per
//! line is appended for every finished iteration (`"record":"iteration"`)
//! and every turn (`"record":"turn"`) of GTP and FlipSight games, so the
//! models can be refitted and replayed offline without new games.
//!
//! Records of one process share `session` (its id); `game` counts the games
//! of the process (see `GameContext`).
use deft_reversi_engine::{Action, Position};
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    sync::{Mutex, OnceLock},
};

static LOG: OnceLock<Option<Mutex<BufWriter<File>>>> = OnceLock::new();

fn log() -> Option<&'static Mutex<BufWriter<File>>> {
    LOG.get_or_init(|| {
        let path = std::env::var_os("DEFT_CLOCK_LOG")?;
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => Some(Mutex::new(BufWriter::new(file))),
            Err(error) => {
                eprintln!(
                    "DEFT_CLOCK_LOG: cannot open {}: {error}",
                    path.to_string_lossy()
                );
                None
            }
        }
    })
    .as_ref()
}

pub(crate) fn enabled() -> bool {
    log().is_some()
}

/// Appends one record; `flush` also writes it out (end of a turn).
pub(crate) fn write(record: &impl Serialize, flush: bool) {
    let Some(log) = log() else {
        return;
    };
    let Ok(mut out) = log.lock() else {
        return;
    };
    // A failing log must not stop the game.
    let _ = serde_json::to_writer(&mut *out, record).map(|()| writeln!(out));
    if flush {
        let _ = out.flush();
    }
}

pub(crate) fn session() -> u32 {
    std::process::id()
}

pub(crate) fn moves(actions: &[Action]) -> Vec<String> {
    actions
        .iter()
        .map(|a| a.to_string().to_lowercase())
        .collect()
}

/// Board features of the side to move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Board {
    pub mobility: u32,
    pub opponent_mobility: u32,
    /// Connected groups of empty squares (4-neighbourhood) and how many of
    /// them have an odd size.
    pub regions: u32,
    pub odd_regions: u32,
}

impl Board {
    pub fn of(pos: Position) -> Self {
        let side = pos.side_to_move();
        let opponent = Position::from_colors(pos.black(), pos.white(), side.opponent())
            .map_or(0, |p| p.legal_moves().count() as u32);
        let (regions, odd_regions) = regions(!(pos.black() | pos.white()));
        Self {
            mobility: pos.legal_moves().count() as u32,
            opponent_mobility: opponent,
            regions,
            odd_regions,
        }
    }
}

/// (regions, odd regions) of the set squares; A1 is bit 0, rows of 8.
fn regions(empty: u64) -> (u32, u32) {
    const FILE_A: u64 = 0x0101_0101_0101_0101;
    const FILE_H: u64 = 0x8080_8080_8080_8080;
    let (mut rest, mut count, mut odd) = (empty, 0, 0);
    while rest != 0 {
        let mut region = rest.isolate_lowest_one();
        loop {
            let grown = (region
                | ((region << 1) & !FILE_A)
                | ((region >> 1) & !FILE_H)
                | (region << 8)
                | (region >> 8))
                & empty;
            if grown == region {
                break;
            }
            region = grown;
        }
        rest &= !region;
        count += 1;
        odd += region.count_ones() % 2;
    }
    (count, odd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_regions_do_not_wrap_around_the_board_edge() {
        // H1 and A2 are adjacent bits but not adjacent squares.
        assert_eq!(regions((1 << 7) | (1 << 8)), (2, 2));
        // A1, B1 and A2 form one region of three.
        assert_eq!(regions(1 | 2 | (1 << 8)), (1, 1));
        assert_eq!(regions(0), (0, 0));
        assert_eq!(regions(u64::MAX), (1, 0));
    }

    #[test]
    fn the_initial_board_has_four_moves_each() {
        assert_eq!(
            Board::of(Position::initial()),
            Board {
                mobility: 4,
                opponent_mobility: 4,
                regions: 1,
                odd_regions: 0,
            }
        );
    }
}
