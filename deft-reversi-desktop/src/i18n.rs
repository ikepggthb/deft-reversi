//! UI strings in Japanese and English. Views read every string from here.
use deft_reversi_engine::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Ja,
    En,
}

pub struct Strings {
    pub new_game: &'static str,
    pub pass: &'static str,
    pub draw: &'static str,
    pub ai_loading: &'static str,
    pub ai_failed: &'static str,
    pub model_missing: &'static str,
    move_number: fn(usize) -> String,
    wins_by: fn(Color, i32) -> String,
}

impl Strings {
    /// The number of the move about to be played.
    pub fn move_number(&self, n: usize) -> String {
        (self.move_number)(n)
    }

    pub fn wins_by(&self, winner: Color, margin: i32) -> String {
        (self.wins_by)(winner, margin)
    }
}

impl Lang {
    /// `DEFT_REVERSI_LANG=en` until the settings screen exists (P2).
    pub fn from_env() -> Self {
        match std::env::var("DEFT_REVERSI_LANG").as_deref() {
            Ok("en") => Self::En,
            _ => Self::Ja,
        }
    }

    pub fn strings(self) -> &'static Strings {
        match self {
            Self::Ja => &JA,
            Self::En => &EN,
        }
    }
}

static JA: Strings = Strings {
    new_game: "新規対局",
    pass: "パス",
    draw: "引き分け",
    ai_loading: "AIを読み込み中…",
    ai_failed: "AIを読み込めませんでした",
    model_missing: "評価関数のファイルが見つかりません",
    move_number: |n| format!("{n}手目"),
    wins_by: |winner, margin| {
        let side = match winner {
            Color::Black => "黒",
            Color::White => "白",
        };
        format!("{side}の{margin}石勝ち")
    },
};

static EN: Strings = Strings {
    new_game: "New game",
    pass: "pass",
    draw: "Draw",
    ai_loading: "Loading the AI…",
    ai_failed: "The AI failed to load",
    model_missing: "Evaluation file not found",
    move_number: |n| format!("Move {n}"),
    wins_by: |winner, margin| {
        let side = match winner {
            Color::Black => "Black",
            Color::White => "White",
        };
        format!("{side} wins by {margin}")
    },
};
