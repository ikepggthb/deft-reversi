use deft_reversi_engine::{Board, Solver, SolverOptions};
use rand::prelude::*;
use rand::rngs::StdRng;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineSide {
    A,
    B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairedSides {
    pub first_black: EngineSide,
    pub second_black: EngineSide,
}

pub fn paired_sides() -> PairedSides {
    PairedSides {
        first_black: EngineSide::A,
        second_black: EngineSide::B,
    }
}

struct Engine {
    solver: Solver,
    think_time: Duration,
    moves: u64,
}

impl Engine {
    fn new(eval_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            solver: Solver::from_file(eval_path, SolverOptions::default())?,
            think_time: Duration::ZERO,
            moves: 0,
        })
    }

    fn choose_move(&mut self, board: &Board, level: i32) -> Option<u8> {
        let start = Instant::now();
        let result = self.solver.solve(board, level);
        self.think_time += start.elapsed();
        self.moves += 1;
        result.best_move
    }

    fn average_ms(&self) -> f64 {
        if self.moves == 0 {
            0.0
        } else {
            self.think_time.as_secs_f64() * 1000.0 / self.moves as f64
        }
    }
}

#[derive(Default)]
struct MatchStats {
    wins: u64,
    draws: u64,
    losses: u64,
    disc_diff_sum: i64,
    games: u64,
}

impl MatchStats {
    fn add_game(&mut self, a_disc_diff: i32) {
        self.games += 1;
        self.disc_diff_sum += i64::from(a_disc_diff);
        match a_disc_diff.cmp(&0) {
            std::cmp::Ordering::Greater => self.wins += 1,
            std::cmp::Ordering::Equal => self.draws += 1,
            std::cmp::Ordering::Less => self.losses += 1,
        }
    }
}

fn random_opening(rng: &mut StdRng, start_rand: usize) -> Board {
    let mut board = Board::new();
    for _ in 0..start_rand {
        let legal = board.moves();
        if legal == 0 {
            if board.opponent_moves() == 0 {
                break;
            }
            board.make_pass();
            continue;
        }
        let n_moves = legal.count_ones();
        let target = rng.gen_range(0..n_moves) as usize;
        let mut bits = legal;
        let mut move_bit = 0;
        for i in 0..=target {
            move_bit = bits & bits.wrapping_neg();
            bits &= bits - 1;
            if i == target {
                break;
            }
        }
        board = board.make_move(move_bit);
    }
    board
}

fn final_disc_diff_for_black(board: &Board, black_to_move: bool) -> i32 {
    let player = board.player.count_ones() as i32;
    let opponent = board.opponent.count_ones() as i32;
    if black_to_move {
        player - opponent
    } else {
        opponent - player
    }
}

fn play_game(
    start_board: Board,
    black_side: EngineSide,
    level: i32,
    engine_a: &mut Engine,
    engine_b: &mut Engine,
) -> i32 {
    let mut board = start_board;
    let mut side_to_move = black_side;
    let mut black_to_move = start_board.move_count() % 2 == 0;

    while board.moves() != 0 || board.opponent_moves() != 0 {
        let legal = board.moves();
        if legal == 0 {
            board.make_pass();
            side_to_move = match side_to_move {
                EngineSide::A => EngineSide::B,
                EngineSide::B => EngineSide::A,
            };
            black_to_move = !black_to_move;
            continue;
        }

        let best_move = match side_to_move {
            EngineSide::A => engine_a.choose_move(&board, level),
            EngineSide::B => engine_b.choose_move(&board, level),
        };
        let Some(best_move) = best_move else {
            break;
        };
        let move_bit = 1u64 << best_move;
        board = board.make_move(move_bit);
        side_to_move = match side_to_move {
            EngineSide::A => EngineSide::B,
            EngineSide::B => EngineSide::A,
        };
        black_to_move = !black_to_move;
    }

    let black_diff = final_disc_diff_for_black(&board, black_to_move);
    match black_side {
        EngineSide::A => black_diff,
        EngineSide::B => -black_diff,
    }
}

pub fn run(
    eval_a: &str,
    eval_b: &str,
    games: usize,
    level: i32,
    start_rand: usize,
    seed: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut engine_a = Engine::new(eval_a)?;
    let mut engine_b = Engine::new(eval_b)?;
    let mut rng = StdRng::seed_from_u64(seed.unwrap_or(0));
    let mut stats = MatchStats::default();

    for _ in 0..games {
        let start_board = random_opening(&mut rng, start_rand);
        stats.add_game(play_game(
            start_board,
            paired_sides().first_black,
            level,
            &mut engine_a,
            &mut engine_b,
        ));
        stats.add_game(play_game(
            start_board,
            paired_sides().second_black,
            level,
            &mut engine_a,
            &mut engine_b,
        ));
    }

    let decisive = stats.wins + stats.losses;
    let win_rate = if stats.games == 0 {
        0.0
    } else {
        (stats.wins as f64 + stats.draws as f64 * 0.5) / stats.games as f64
    };
    let avg_disc_diff = if stats.games == 0 {
        0.0
    } else {
        stats.disc_diff_sum as f64 / stats.games as f64
    };

    println!("definition,pairs,total_games");
    println!("games_is_pairs,{games},{}", stats.games);
    println!("a_wins,draws,a_losses,decisive_games,win_rate,avg_disc_diff");
    println!(
        "{},{},{},{},{:.4},{:.3}",
        stats.wins, stats.draws, stats.losses, decisive, win_rate, avg_disc_diff
    );
    println!("engine,avg_think_ms,moves");
    println!("A,{:.3},{}", engine_a.average_ms(), engine_a.moves);
    println!("B,{:.3},{}", engine_b.average_ms(), engine_b.moves);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_match_swaps_black_side() {
        let sides = paired_sides();
        assert_eq!(sides.first_black, EngineSide::A);
        assert_eq!(sides.second_black, EngineSide::B);
    }
}
