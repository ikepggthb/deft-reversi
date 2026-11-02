//! UI strings in Japanese and English. Views read every string from here.
use crate::domain::Preset;
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
    pub thinking: &'static str,
    pub paused: &'static str,
    pub pause: &'static str,
    pub resume: &'static str,
    pub step_ai: &'static str,
    pub move_now: &'static str,
    pub human: &'static str,
    pub ai: &'static str,
    pub first_player: &'static str,
    pub swap: &'static str,
    pub manual: &'static str,
    pub by_level: &'static str,
    pub by_time: &'static str,
    pub start_position: &'static str,
    pub initial_position: &'static str,
    pub cancel: &'static str,
    pub start_game: &'static str,
    pub empties: &'static str,
    pub selective_endgame: &'static str,
    pub exact: &'static str,
    pub no_pruning: &'static str,
    pub time_note: &'static str,
    pub settings: &'static str,
    pub board_group: &'static str,
    pub appearance: &'static str,
    pub engine_group: &'static str,
    pub legal_moves: &'static str,
    pub show: &'static str,
    pub hide: &'static str,
    pub language: &'static str,
    pub threads: &'static str,
    pub hash_table: &'static str,
    pub close: &'static str,
    move_number: fn(usize) -> String,
    wins_by: fn(Color, i32) -> String,
    color: fn(Color) -> &'static str,
    preset: fn(Preset) -> &'static str,
    current_position: fn(usize) -> String,
    manual_title: fn(Color) -> String,
    depth: fn(u8) -> String,
    seconds: fn(u64) -> String,
    logical_cpus: fn(usize) -> String,
}

impl Strings {
    /// The number of the move about to be played.
    pub fn move_number(&self, n: usize) -> String {
        (self.move_number)(n)
    }
    pub fn wins_by(&self, winner: Color, margin: i32) -> String {
        (self.wins_by)(winner, margin)
    }
    pub fn color(&self, color: Color) -> &'static str {
        (self.color)(color)
    }
    pub fn preset(&self, preset: Preset) -> &'static str {
        (self.preset)(preset)
    }
    pub fn current_position(&self, move_number: usize) -> String {
        (self.current_position)(move_number)
    }
    pub fn manual_title(&self, color: Color) -> String {
        (self.manual_title)(color)
    }
    /// "so many moves ahead"
    pub fn depth(&self, depth: u8) -> String {
        (self.depth)(depth)
    }
    pub fn seconds(&self, seconds: u64) -> String {
        (self.seconds)(seconds)
    }
    pub fn logical_cpus(&self, n: usize) -> String {
        (self.logical_cpus)(n)
    }
}

impl Lang {
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
    thinking: "考え中",
    paused: "一時停止中",
    pause: "一時停止",
    resume: "再開",
    step_ai: "1手進める",
    move_now: "今すぐ打つ",
    human: "人間",
    ai: "AI",
    first_player: "先手",
    swap: "入れ替え",
    manual: "マニュアル",
    by_level: "レベル",
    by_time: "1手の時間",
    start_position: "開始局面",
    initial_position: "初期配置",
    cancel: "キャンセル",
    start_game: "対局開始",
    empties: "空き",
    selective_endgame: "選択的に終局まで",
    exact: "完全読み",
    no_pruning: "全探索",
    time_note: "深さを決めずに読み進め、時間になった時点の最善手を打ちます。",
    settings: "設定",
    board_group: "盤面",
    appearance: "外観",
    engine_group: "エンジン",
    legal_moves: "合法手",
    show: "表示",
    hide: "非表示",
    language: "言語",
    threads: "スレッド数",
    hash_table: "置換表",
    close: "閉じる",
    move_number: |n| format!("{n}手目"),
    wins_by: |winner, margin| {
        let side = (JA.color)(winner);
        format!("{side}の{margin}石勝ち")
    },
    color: |color| match color {
        Color::Black => "黒",
        Color::White => "白",
    },
    preset: |preset| match preset {
        Preset::Beginner => "初級",
        Preset::Intermediate => "中級",
        Preset::Advanced => "上級",
        Preset::Expert => "達人",
        Preset::Strongest => "最強",
    },
    current_position: |n| format!("現在の局面（{n}手目）"),
    manual_title: |color| format!("{}のマニュアル設定", (JA.color)(color)),
    depth: |depth| format!("{depth}手読み"),
    seconds: |s| format!("{s} 秒"),
    logical_cpus: |n| format!("論理CPU {n}"),
};

static EN: Strings = Strings {
    new_game: "New game",
    pass: "pass",
    draw: "Draw",
    ai_loading: "Loading the AI…",
    ai_failed: "The AI failed to load",
    model_missing: "Evaluation file not found",
    thinking: "Thinking",
    paused: "Paused",
    pause: "Pause",
    resume: "Resume",
    step_ai: "One move",
    move_now: "Move now",
    human: "Human",
    ai: "AI",
    first_player: "first",
    swap: "Swap",
    manual: "Manual",
    by_level: "Level",
    by_time: "Time per move",
    start_position: "Start from",
    initial_position: "Initial position",
    cancel: "Cancel",
    start_game: "Start",
    empties: "Empties",
    selective_endgame: "To the end, selectively",
    exact: "Exact solve",
    no_pruning: "full",
    time_note: "Reads deeper until the time is up, then plays its best move so far.",
    settings: "Settings",
    board_group: "Board",
    appearance: "Appearance",
    engine_group: "Engine",
    legal_moves: "Legal moves",
    show: "Show",
    hide: "Hide",
    language: "Language",
    threads: "Threads",
    hash_table: "Hash table",
    close: "Close",
    move_number: |n| format!("Move {n}"),
    wins_by: |winner, margin| format!("{} wins by {margin}", (EN.color)(winner)),
    color: |color| match color {
        Color::Black => "Black",
        Color::White => "White",
    },
    preset: |preset| match preset {
        Preset::Beginner => "Beginner",
        Preset::Intermediate => "Intermediate",
        Preset::Advanced => "Advanced",
        Preset::Expert => "Expert",
        Preset::Strongest => "Strongest",
    },
    current_position: |n| format!("Current position (move {n})"),
    manual_title: |color| format!("Manual settings for {}", (EN.color)(color)),
    depth: |depth| format!("{depth} moves ahead"),
    seconds: |s| format!("{s} s"),
    logical_cpus: |n| format!("{n} logical CPUs"),
};
