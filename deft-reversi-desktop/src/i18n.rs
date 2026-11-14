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
    pub file_menu: &'static str,
    pub open: &'static str,
    pub export: &'static str,
    pub games: &'static str,
    pub delete: &'static str,
    pub delete_confirm: &'static str,
    pub in_progress: &'static str,
    pub no_games: &'static str,
    pub already_running: &'static str,
    pub analysis: &'static str,
    pub exact_solve: &'static str,
    pub analyze_record: &'static str,
    pub stop: &'static str,
    pub analyzing: &'static str,
    pub moves_incomplete: &'static str,
    pub candidate_values: &'static str,
    pub numbers: &'static str,
    pub dots: &'static str,
    pub in_analysis: &'static str,
    pub analysis_level: &'static str,
    pub analysis_time: &'static str,
    pub batch_time: &'static str,
    pub loss_threshold: &'static str,
    pub discs_or_more: &'static str,
    pub edit_position: &'static str,
    pub start_from_here: &'static str,
    pub erase: &'static str,
    pub side_to_move: &'static str,
    pub initial_setup: &'static str,
    pub clear_all: &'static str,
    pub paint_hint: &'static str,
    pub copy: &'static str,
    pub paste: &'static str,
    pub paste_failed: &'static str,
    pub too_few_discs: &'static str,
    pub finished_position: &'static str,
    move_number: fn(usize) -> String,
    wins_by: fn(Color, i32) -> String,
    color: fn(Color) -> &'static str,
    preset: fn(Preset) -> &'static str,
    current_position: fn(usize) -> String,
    manual_title: fn(Color) -> String,
    depth: fn(u8) -> String,
    seconds: fn(u64) -> String,
    logical_cpus: fn(usize) -> String,
    moves: fn(usize) -> String,
    exported: fn(&str) -> String,
    load_failed: fn(&str) -> String,
    save_failed: fn(&str) -> String,
    pass_start: fn(Color) -> String,
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
    pub fn moves(&self, n: usize) -> String {
        (self.moves)(n)
    }
    pub fn exported(&self, path: &str) -> String {
        (self.exported)(path)
    }
    pub fn load_failed(&self, error: &str) -> String {
        (self.load_failed)(error)
    }
    pub fn save_failed(&self, error: &str) -> String {
        (self.save_failed)(error)
    }
    /// The side to move cannot, so the other one starts.
    pub fn pass_start(&self, side: Color) -> String {
        (self.pass_start)(side)
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
    file_menu: "ファイル",
    open: "開く…",
    export: "書き出し…",
    games: "対局一覧",
    delete: "削除",
    delete_confirm: "削除する",
    in_progress: "対局中",
    no_games: "保存された対局はありません",
    already_running: "deft-reversi はすでに起動しています。",
    analysis: "解析",
    exact_solve: "完全読み",
    analyze_record: "全棋譜を解析",
    stop: "停止",
    analyzing: "解析中",
    moves_incomplete: "候補評価未完了",
    candidate_values: "候補手の評価値",
    numbers: "数値",
    dots: "青丸",
    in_analysis: "解析モードのとき",
    analysis_level: "解析の強さ",
    analysis_time: "1局面の上限時間",
    batch_time: "全棋譜の1局面",
    loss_threshold: "悪手として示す損失",
    discs_or_more: "石以上",
    edit_position: "局面編集",
    start_from_here: "この局面で始める",
    erase: "消す",
    side_to_move: "手番",
    initial_setup: "初期配置",
    clear_all: "全消去",
    paint_hint: "ドラッグで続けて置ける
右クリックで消す",
    copy: "コピー",
    paste: "貼り付け",
    paste_failed: "貼り付けた文字列は局面として読めません。",
    too_few_discs: "石が4個未満の局面では、AIと解析は使えません。",
    finished_position: "どちらも打てないため、終局した局面になります。",
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
    moves: |n| format!("{n}手"),
    exported: |path| format!("書き出しました: {path}"),
    load_failed: |error| format!("開けませんでした: {error}"),
    save_failed: |error| format!("保存できませんでした: {error}"),
    pass_start: |side| {
        let other = (JA.color)(side.opponent());
        format!(
            "{}に打てる手がないため、{other}番から始めます。",
            (JA.color)(side)
        )
    },
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
    file_menu: "File",
    open: "Open…",
    export: "Export…",
    games: "Games",
    delete: "Delete",
    delete_confirm: "Delete it",
    in_progress: "In progress",
    no_games: "No saved games",
    already_running: "deft-reversi is already running.",
    analysis: "Analysis",
    exact_solve: "Solve exactly",
    analyze_record: "Analyse the record",
    stop: "Stop",
    analyzing: "Analysing",
    moves_incomplete: "Move scores incomplete",
    candidate_values: "Candidate values",
    numbers: "Numbers",
    dots: "Dots",
    in_analysis: "in analysis mode",
    analysis_level: "Analysis level",
    analysis_time: "Time per position",
    batch_time: "Record, per position",
    loss_threshold: "Mark moves losing",
    discs_or_more: "discs or more",
    edit_position: "Edit position",
    start_from_here: "Start from here",
    erase: "Erase",
    side_to_move: "To move",
    initial_setup: "Initial",
    clear_all: "Clear",
    paint_hint: "Drag to keep placing
Right-click to erase",
    copy: "Copy",
    paste: "Paste",
    paste_failed: "The pasted text is not a position.",
    too_few_discs: "With fewer than 4 discs, the AI and analysis are off.",
    finished_position: "Neither side can move: the game is over here.",
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
    moves: |n| format!("{n} moves"),
    exported: |path| format!("Exported to {path}"),
    load_failed: |error| format!("Could not open: {error}"),
    save_failed: |error| format!("Could not save: {error}"),
    pass_start: |side| {
        format!(
            "{} cannot move, so {} starts.",
            (EN.color)(side),
            (EN.color)(side.opponent())
        )
    },
};
