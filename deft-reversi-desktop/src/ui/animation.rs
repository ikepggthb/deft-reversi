//! The turn-over animation (§9.3). It is drawn over the finished position
//! and never touches the game: a new move, Undo or loading drops it.
use deft_reversi_engine::{Color, Position, Square};
use std::time::Instant;

/// The placed disc grows in this long…
const PLACE_MS: f32 = 140.;
/// …then each turned disc flips in this long, the nearest first.
const FLIP_MS: f32 = 260.;
const STAGGER_MS: f32 = 35.;

pub(super) struct FlipAnimation {
    placed: Square,
    /// Each turned disc and when it starts, in ms.
    flips: Vec<(Square, f32)>,
    /// The colour the turned discs had.
    from: Color,
    start: Instant,
    /// 1 for normal, 0.5 for short.
    scale: f32,
    length_ms: f32,
}

/// How to draw one square while the animation runs.
pub(super) enum Frame {
    /// The new disc at this size, 0 to 1.
    Growing(f32),
    /// A turned disc: its width, 0 to 1, and the colour it shows.
    Turning(f32, Color),
}

impl FlipAnimation {
    /// The move `square` played in `before`; None when it is not a move.
    pub fn new(before: Position, square: Square, scale: f32) -> Option<Self> {
        let flips = before.flips(square);
        if flips == 0 || scale <= 0. {
            return None;
        }
        let distance = |s: Square| {
            let dx = (i32::from(s.x()) - i32::from(square.x())).abs();
            let dy = (i32::from(s.y()) - i32::from(square.y())).abs();
            dx.max(dy) as f32
        };
        let flips: Vec<(Square, f32)> = (0..64u8)
            .filter_map(Square::new)
            .filter(|s| flips & s.bit() != 0)
            .map(|s| {
                (
                    s,
                    (PLACE_MS * 0.5 + STAGGER_MS * (distance(s) - 1.)) * scale,
                )
            })
            .collect();
        let last = flips.iter().map(|&(_, at)| at).fold(0., f32::max);
        Some(Self {
            placed: square,
            flips,
            from: before.side_to_move().opponent(),
            start: Instant::now(),
            scale,
            length_ms: last + FLIP_MS * scale,
        })
    }

    fn elapsed_ms(&self) -> f32 {
        self.start.elapsed().as_secs_f32() * 1000.
    }

    pub fn done(&self) -> bool {
        self.elapsed_ms() >= self.length_ms
    }

    #[cfg(test)]
    pub fn length(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f32(self.length_ms / 1000.)
    }

    pub fn frame(&self, square: Square) -> Option<Frame> {
        let now = self.elapsed_ms();
        if square == self.placed {
            let t = (now / (PLACE_MS * self.scale)).clamp(0., 1.);
            return (t < 1.).then_some(Frame::Growing(0.55 + 0.45 * ease_out(t)));
        }
        let &(_, at) = self.flips.iter().find(|(s, _)| *s == square)?;
        let t = ((now - at) / (FLIP_MS * self.scale)).clamp(0., 1.);
        if t >= 1. {
            return None;
        }
        // Turns about its axis: the width a turning disc shows, eased so it
        // starts and ends softly; the colour changes when it is edge-on.
        let t = ease_in_out(t);
        let colour = if t < 0.5 {
            self.from
        } else {
            self.from.opponent()
        };
        let width = (std::f32::consts::PI * t).cos().abs();
        Some(Frame::Turning(width.max(0.06), colour))
    }
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_move_animates_its_flips_and_ends() {
        let square: Square = "f5".parse().unwrap();
        let animation = FlipAnimation::new(Position::initial(), square, 1.).unwrap();
        assert_eq!(animation.flips.len(), 1);
        assert_eq!(animation.from, Color::White);
        assert!(animation.length() <= Duration::from_millis(400));
        assert!(matches!(animation.frame(square), Some(Frame::Growing(_))));
        assert!(animation.frame("a1".parse().unwrap()).is_none());
        let short = FlipAnimation::new(Position::initial(), square, 0.5).unwrap();
        assert!(short.length() < animation.length());
        assert!(FlipAnimation::new(Position::initial(), "a1".parse().unwrap(), 1.).is_none());
    }
}
