//! 局面エディタ用の下書き状態を保持するモジュール。
//!
//! 実際の対局状態とは独立して編集を保持するため、キャンセルしても元の対局には影響しない。
use deft_reversi_engine::{Color, Position, Square};

/// 評価関数が前提とする最小石数。
///
/// 通常の初期局面は 4 石から始まるため、これ未満の石数の局面は評価モデルの学習範囲外となる。
/// そのため表示や保存はできても、AI の思考や局面解析の対象からは除外する。
pub const MIN_SEARCH_DISCS: u32 = 4;

/// 指定された局面が探索・解析可能（石数が [`MIN_SEARCH_DISCS`] 以上）かを返す。
pub fn searchable(position: Position) -> bool {
    position.disc_count(Color::Black) + position.disc_count(Color::White) >= MIN_SEARCH_DISCS
}

/// 盤面エディタでマスに描画するペンの種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pen {
    /// 黒石を置く。
    Black,
    /// 白石を置く。
    White,
    /// 石を消去する。
    Erase,
}

/// 局面エディタで編集中の盤面下書き。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorDraft {
    /// 黒石の配置（64 マスを `u64` の各ビットに対応させたビットボード表現）。
    black: u64,
    /// 白石の配置（同上のビットボード表現）。
    white: u64,
    /// 手番の色。
    pub side: Color,
}

/// 下書き局面から対局・解析を開始する際の注意点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caveat {
    /// 手番側に合法手がなく、パス後に相手番から始まる。
    Pass(Color),
    /// 双方ともに合法手がなく、すでに終局している。
    Finished,
    /// 石数が [`MIN_SEARCH_DISCS`] 未満のため AI・解析が無効になる。
    TooFewDiscs,
}

impl EditorDraft {
    /// 既存の [`Position`] から下書きを作成する。
    pub fn from_position(position: Position) -> Self {
        Self {
            black: position.black(),
            white: position.white(),
            side: position.side_to_move(),
        }
    }

    /// 通常の初期局面から下書きを作成する。
    pub fn initial() -> Self {
        Self::from_position(Position::initial())
    }

    /// 石が 1 つもない空盤面の下書きを作成する。
    pub fn empty(side: Color) -> Self {
        Self {
            black: 0,
            white: 0,
            side,
        }
    }

    /// 指定マスに現在のペンを適用する（既存の石は上書き・消去される）。
    pub fn paint(&mut self, square: Square, pen: Pen) {
        let bit = square.bit();
        self.black &= !bit;
        self.white &= !bit;
        match pen {
            Pen::Black => self.black |= bit,
            Pen::White => self.white |= bit,
            Pen::Erase => {}
        }
    }

    /// 下書きから [`Position`] を構築する。
    ///
    /// `paint` 時に両色のビットが重ならないよう排他更新しているため、常に有効な局面になる。
    pub fn position(&self) -> Position {
        Position::from_colors(self.black, self.white, self.side).expect("colours are disjoint")
    }

    /// 現在の下書き局面に対する注意点（パス・終局・石数不足）を判定する。
    pub fn caveat(&self) -> Option<Caveat> {
        let position = self.position();
        if !searchable(position) {
            Some(Caveat::TooFewDiscs)
        } else if position.is_terminal() {
            Some(Caveat::Finished)
        } else if position.must_pass() {
            Some(Caveat::Pass(self.side))
        } else {
            None
        }
    }

    /// エンジン標準の局面テキスト（A1 から 64 文字の `X`/`O`/`-` ＋スペース＋手番）を返す。
    pub fn text(&self) -> String {
        self.position().to_string()
    }

    /// エンジン標準の局面テキストを解析して下書きを生成する（前後の空白・改行は無視）。
    pub fn parse(text: &str) -> Option<Self> {
        text.trim()
            .parse::<Position>()
            .ok()
            .map(Self::from_position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(name: &str) -> Square {
        name.parse().unwrap()
    }

    #[test]
    fn painting_text_and_caveats() {
        let mut draft = EditorDraft::initial();
        assert_eq!(draft.caveat(), None);
        assert_eq!(
            EditorDraft::parse(&format!("  {}\n", draft.text())),
            Some(draft)
        );
        assert_eq!(EditorDraft::parse("not a position"), None);

        // Painting replaces whatever was on the square.
        draft.paint(square("d4"), Pen::Black);
        assert_eq!(draft.position().disc_count(Color::Black), 3);
        draft.paint(square("d4"), Pen::Erase);
        assert_eq!(draft.position().disc_count(Color::White), 1);

        let mut draft = EditorDraft::empty(Color::Black);
        draft.paint(square("a1"), Pen::Black);
        draft.paint(square("b1"), Pen::White);
        assert_eq!(draft.caveat(), Some(Caveat::TooFewDiscs));

        // Black A1, D1; White B1, A2; White to move cannot, Black can.
        draft.paint(square("d1"), Pen::Black);
        draft.paint(square("a2"), Pen::White);
        draft.side = Color::White;
        assert_eq!(draft.caveat(), Some(Caveat::Pass(Color::White)));

        // Only black discs: nobody can move.
        let mut draft = EditorDraft::empty(Color::Black);
        for name in ["a1", "b1", "c1", "d1"] {
            draft.paint(square(name), Pen::Black);
        }
        assert_eq!(draft.caveat(), Some(Caveat::Finished));
    }
}
