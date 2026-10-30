//! Root PVS, per-move bounds and aspiration stabilization from the reference.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct RootBound {
    lower: i32,
    upper: i32,
}
impl RootBound {
    const UNBOUNDED: Self = Self {
        lower: -64,
        upper: 64,
    };
    fn cutoff(self, alpha: i32, beta: i32) -> Option<i32> {
        if self.lower == self.upper || self.lower >= beta {
            Some(self.lower)
        } else if self.upper <= alpha {
            Some(self.upper)
        } else {
            None
        }
    }
    pub(super) fn update(&mut self, score: i32, alpha: i32, beta: i32) -> i32 {
        if score <= alpha {
            self.upper = self.upper.min(score);
        } else if score >= beta {
            self.lower = self.lower.max(score);
        } else {
            self.lower = score;
            self.upper = score;
        }
        debug_assert!(self.lower <= self.upper);
        if self.lower == self.upper {
            self.lower
        } else {
            score
        }
    }
}

pub(super) struct RootSearch {
    position: Position,
    candidates: Vec<Candidate>,
    bounds: [RootBound; 64],
}
impl RootSearch {
    pub(super) fn new(position: Position) -> Self {
        let mut moves = position.board.get_moves();
        let mut candidates = Vec::new();
        while moves != 0 {
            let bit = moves.isolate_lowest_one();
            moves &= moves - 1;
            candidates.push(Candidate {
                square: Square::lowest(bit),
                flips: position.board.flip_mask(bit),
                ordering: 0,
            });
        }
        Self {
            position,
            candidates,
            bounds: [RootBound::UNBOUNDED; 64],
        }
    }
    pub(super) fn iteration(
        &mut self,
        context: &mut Context,
        spec: SearchSpec,
        prediction: i32,
    ) -> Result<Line, Aborted> {
        self.bounds.fill(RootBound::UNBOUNDED);
        let previous = self.candidates.first().copied();
        let outcome = self.aspiration(context, spec, prediction);
        if outcome.is_err()
            && let Some(previous) = previous
            && let Some(index) = self
                .candidates
                .iter()
                .position(|c| c.square == previous.square)
        {
            self.candidates.swap(0, index);
        }
        outcome
    }
    /// Scores every root move with a full window, best first.
    pub(super) fn move_scores(
        &self,
        context: &mut Context,
        spec: SearchSpec,
    ) -> Result<Vec<MoveScore>, Aborted> {
        let mut moves = Vec::with_capacity(self.candidates.len());
        for candidate in &self.candidates {
            let child = context.node(
                candidate.child(self.position, true),
                -64,
                64,
                SearchSpec {
                    depth: spec.depth - 1,
                    ..spec
                },
            )?;
            moves.push(MoveScore {
                action: candidate.action(),
                score: -child.score,
            });
        }
        moves.sort_by_key(|entry| std::cmp::Reverse(entry.score));
        Ok(moves)
    }
    fn aspiration(
        &mut self,
        context: &mut Context,
        spec: SearchSpec,
        prediction: i32,
    ) -> Result<Line, Aborted> {
        if spec.depth <= 10 {
            return self.window(context, spec, -64, 64);
        }
        let radius = if spec.exact {
            (10 - i32::from(self.position.board.empty_count())).max(2 + prediction.rem_euclid(2))
        } else if spec.depth > 16 {
            2
        } else {
            6
        };
        let mut score = prediction;
        let mut best = Line::leaf(score);
        for retry in 0..10 {
            let previous = score;
            let width = retry.max(1) * radius;
            let (mut left, mut right) = (width, width);
            loop {
                let (alpha, beta) = aspiration_window(score, left, right, spec.exact);
                let (alpha, beta) = (alpha.max(-64), beta.min(64));
                if alpha >= beta {
                    break;
                }
                best = self.window(context, spec, alpha, beta)?;
                score = best.score;
                if (score <= -64 && alpha == -64) || (score >= 64 && beta == 64) {
                    break;
                }
                if score <= alpha && left != 0 {
                    left = (left * 2).min(128);
                    right = 0;
                } else if score >= beta && right != 0 {
                    right = (right * 2).min(128);
                    left = 0;
                } else {
                    break;
                }
            }
            if score == previous {
                break;
            }
        }
        Ok(best)
    }
    fn candidate(
        &mut self,
        context: &mut Context,
        index: usize,
        spec: SearchSpec,
        alpha: i32,
        beta: i32,
    ) -> Result<Line, Aborted> {
        let candidate = self.candidates[index];
        let bound = &mut self.bounds[candidate.square.index() as usize];
        let reuse = spec.exact && context.runtime.sigma().is_none();
        if reuse && let Some(score) = bound.cutoff(alpha, beta) {
            return Ok(root_line(candidate.action(), score));
        }
        let (low, high) = if reuse {
            (alpha.max(bound.lower), beta.min(bound.upper))
        } else {
            (alpha, beta)
        };
        let child = context.node(
            candidate.child(self.position, true),
            -high,
            -low,
            SearchSpec {
                depth: spec.depth - 1,
                ..spec
            },
        )?;
        let mut line = Line::prepend(candidate.action(), &child);
        if reuse {
            line.score = bound.update(line.score, low, high);
        }
        Ok(line)
    }
    fn window(
        &mut self,
        context: &mut Context,
        spec: SearchSpec,
        mut alpha: i32,
        beta: i32,
    ) -> Result<Line, Aborted> {
        if context.aborted() {
            return Err(Aborted);
        }
        if self.candidates.is_empty() || spec.depth == 0 {
            return context.node(self.position, alpha, beta, spec);
        }
        context.enter()?;
        let reuse = spec.exact && context.runtime.sigma().is_none();
        let mut best = self.candidate(context, 0, spec, alpha, beta)?;
        if best.score >= beta {
            return Ok(best);
        }
        alpha = alpha.max(best.score);
        // Keep YBWC at the root; workers share the same score-only TT rules.
        let split = context.runtime.parallel
            && self.candidates.len() >= 3
            && if spec.exact {
                self.position.board.empty_count() >= tree::SPLIT_EMPTIES
            } else {
                spec.depth >= tree::SPLIT_DEPTH
            };
        if split && context.runtime.pool.can_offer(&context.ancestors) {
            let bounds = reuse.then(|| Arc::new(Mutex::new(self.bounds)));
            let line = context.split(
                self.position,
                self.candidates[1..].to_vec(),
                (alpha, beta),
                SearchSpec {
                    depth: spec.depth - 1,
                    ..spec
                },
                best,
                bounds.clone(),
            )?;
            if let Some(bounds) = bounds {
                self.bounds = *parallel::lock(&bounds);
            }
            if let Some(action) = line.actions[0]
                && let Some(index) = self.candidates.iter().position(|c| c.action() == action)
            {
                self.candidates.swap(0, index);
            }
            return Ok(line);
        }
        let mut best_index = 0;
        for index in 1..self.candidates.len() {
            if context.aborted() {
                return Err(Aborted);
            }
            let candidate = self.candidates[index];
            let bound = self.bounds[candidate.square.index() as usize];
            if reuse && bound.lower >= beta {
                self.candidates.swap(0, index);
                return Ok(root_line(candidate.action(), bound.lower));
            }
            if reuse && bound.upper <= alpha {
                if bound.upper > best.score {
                    best = root_line(candidate.action(), bound.upper);
                    best_index = index;
                }
                continue;
            }
            let child = context.node(
                candidate.child(self.position, true),
                -alpha - 1,
                -alpha,
                SearchSpec {
                    depth: spec.depth - 1,
                    ..spec
                },
            )?;
            let mut line = Line::prepend(candidate.action(), &child);
            if reuse {
                line.score = self.bounds[candidate.square.index() as usize].update(
                    line.score,
                    alpha,
                    alpha + 1,
                );
            }
            if line.score >= beta {
                self.candidates.swap(0, index);
                return Ok(line);
            }
            if line.score > alpha {
                line = self.candidate(context, index, spec, alpha, beta)?;
                if line.score >= beta {
                    self.candidates.swap(0, index);
                    return Ok(line);
                }
                if line.score > alpha {
                    alpha = line.score;
                    best = line;
                    best_index = index;
                }
            }
        }
        self.candidates.swap(0, best_index);
        Ok(best)
    }
}
fn root_line(action: Action, score: i32) -> Line {
    let mut line = Line::leaf(score);
    line.actions[0] = Some(action);
    line.len = 1;
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn root_bounds_combine_research_and_reset_between_iterations() {
        let mut bound = RootBound::UNBOUNDED;
        assert_eq!(bound.update(8, 0, 4), 8);
        assert_eq!(bound.cutoff(0, 6), Some(8));
        assert_eq!(bound.cutoff(0, 10), None);
        assert_eq!(bound.update(8, 8, 10), 8);
        assert_eq!((bound.lower, bound.upper), (8, 8));
        let board = Board::new();
        let mut root = RootSearch::new(Position {
            board,
            state: EvalState::from_board(board),
        });
        root.bounds.fill(bound);
        let mut context = super::super::session::test_context();
        let line = root
            .iteration(
                &mut context,
                SearchSpec {
                    depth: 3,
                    exact: false,
                    probcut: true,
                },
                30,
            )
            .unwrap();
        assert_eq!(line.score, 0);
        assert_eq!(root.bounds[19].lower, -64);
    }
}
