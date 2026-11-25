//! 石を置いてから挟んだ石が順に裏返る反転アニメーションを管理するモジュール。
//!
//! 確定後の局面の上に視覚効果として重ね描きするだけであり、対局状態には一切干渉しない
//! （次の着手・Undo・棋譜読み込みが発生すると即座に破棄される）。
use deft_reversi_engine::{Color, Position, Square};
use std::time::Instant;

/// 着手した石が拡大表示される時間（ms）。
const PLACE_MS: f32 = 140.;
/// 裏返る石 1 つあたりの反転時間（ms）。
const FLIP_MS: f32 = 260.;
/// 着手位置から縦横斜め何マス離れているか（チェビシェフ距離 `max(|dx|, |dy|)`）に応じた反転開始の遅延間隔（ms）。
/// 遠い石ほど少し遅れて裏返り始めることで、波紋のように広がる自然な動きになる。
const STAGGER_MS: f32 = 35.;

/// 1 手分の石配置・反転アニメーション状態。
pub(super) struct FlipAnimation {
    placed: Square,
    /// 反転する各石と、その反転開始時刻（アニメーション開始からの ms）。
    flips: Vec<(Square, f32)>,
    /// 反転前の石の色。
    from: Color,
    start: Instant,
    /// 再生速度倍率（標準 1.0、高速 0.5）。
    scale: f32,
    length_ms: f32,
}

/// アニメーション実行中の特定マスにおける描画フレーム状態。
pub(super) enum Frame {
    /// 新しく置かれた石の拡大率（0.0〜1.0）。
    Growing(f32),
    /// 反転中の石の見かけの幅（0.0〜1.0）と現在見えている面の石色。
    Turning(f32, Color),
}

impl FlipAnimation {
    /// `before` の局面で `square` に打ったときのアニメーションを生成する（非合法手や無効時は `None`）。
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

    /// アニメーションが終了したかを返す。
    pub fn done(&self) -> bool {
        self.elapsed_ms() >= self.length_ms
    }

    /// アニメーション全体の長さ（テスト用）。
    #[cfg(test)]
    pub fn length(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f32(self.length_ms / 1000.)
    }

    /// 現在時刻における指定マス `square` のアニメーションフレームを返す。
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
        // 軸を中心に回転する見かけの幅をコサインで表現し、真横（t = 0.5）を通過した瞬間に色を切り替える。
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
