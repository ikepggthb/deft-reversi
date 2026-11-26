//! UI 文言の多言語（日本語 / 英語）定義を提供するモジュール。
//!
//! すべてのビューはこのモジュールの [`Strings`] を通じて表示テキストを取得する。
use crate::domain::Preset;
use deft_reversi_engine::Color;

/// サポートする表示言語。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    /// 日本語。
    Ja,
    /// 英語。
    En,
}

/// 選択中の言語に対応する UI 文言テーブル。
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
    pub exact_mark: &'static str,
    pub book: &'static str,
    pub book_group: &'static str,
    pub book_file: &'static str,
    pub book_none: &'static str,
    pub book_choose: &'static str,
    pub book_remove: &'static str,
    pub book_use: &'static str,
    pub book_note: &'static str,
    pub book_loading: &'static str,
    pub book_other_model: &'static str,
    pub book_failed: &'static str,
    pub version_group: &'static str,
    pub version_app: &'static str,
    pub version_book_lib: &'static str,
    pub version_model: &'static str,
    pub version_not_loaded: &'static str,
    pub analysis_id: &'static str,
    pub book_written_by: &'static str,
    pub book_grown_with: &'static str,
    pub book_model_label: &'static str,
    pub book_work: &'static str,
    pub book_work_file: &'static str,
    pub book_new: &'static str,
    pub book_open: &'static str,
    pub book_from: &'static str,
    pub book_from_current: &'static str,
    pub book_level: &'static str,
    pub book_max_plies: &'static str,
    pub book_plies_note: &'static str,
    pub book_loss: &'static str,
    pub book_loss_note: &'static str,
    pub book_cumulative: &'static str,
    pub book_cumulative_note: &'static str,
    pub book_no_limit: &'static str,
    pub book_exhausted: &'static str,
    pub book_memory: &'static str,
    pub book_stopped: &'static str,
    pub book_start: &'static str,
    pub book_stop: &'static str,
    pub book_generating: &'static str,
    pub book_updated: &'static str,
    pub book_other_work: &'static str,
    pub book_moves: &'static str,
    pub book_link: &'static str,
    pub book_unlink: &'static str,
    pub book_not_in: &'static str,
    pub book_work_note: &'static str,
    pub book_from_initial: &'static str,
    pub book_from_record: &'static str,
    pub book_leaf: &'static str,
    pub book_leaf_note: &'static str,
    pub book_fix: &'static str,
    pub book_fix_note: &'static str,
    pub book_fixing: &'static str,
    pub book_fixed: &'static str,
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
    pub study: &'static str,
    pub studying: &'static str,
    pub main_line: &'static str,
    pub study_line: &'static str,
    pub back_to_main: &'static str,
    pub exact_solve: &'static str,
    pub analyze_record: &'static str,
    pub reanalyze: &'static str,
    pub stop: &'static str,
    pub analyzing: &'static str,
    pub candidate_values: &'static str,
    pub analysis_level: &'static str,
    pub analysis_heading: &'static str,
    pub value_label: &'static str,
    pub best_move: &'static str,
    pub expected_line: &'static str,
    pub reading_label: &'static str,
    pub candidates: &'static str,
    pub graph_title: &'static str,
    pub black_ahead: &'static str,
    pub white_ahead: &'static str,
    pub start_label: &'static str,
    pub record_heading: &'static str,
    pub opening_heading: &'static str,
    pub game_finished: &'static str,
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
    pub theme: &'static str,
    pub theme_system: &'static str,
    pub theme_light: &'static str,
    pub theme_dark: &'static str,
    pub flip_animation: &'static str,
    pub animation_normal: &'static str,
    pub animation_short: &'static str,
    pub animation_off: &'static str,
    pub sound: &'static str,
    pub on: &'static str,
    pub off: &'static str,
    pub copy_record: &'static str,
    pub enter_record: &'static str,
    pub record_copied: &'static str,
    pub load: &'static str,
    pub record_example: &'static str,
    pub ai_log: &'static str,
    pub turn_board: &'static str,
    pub mirror_board: &'static str,
    pub no_ai_moves: &'static str,
    pub log_number: &'static str,
    pub log_move: &'static str,
    pub log_reading: &'static str,
    pub log_time: &'static str,
    pub log_nodes: &'static str,
    pub log_strength: &'static str,
    lang: Lang,
}

impl Strings {
    /// これから打たれる手数（例: `"5手目"` / `"Move 5"`）を返す。
    pub fn move_number(&self, n: usize) -> String {
        match self.lang {
            Lang::Ja => format!("{n}手目"),
            Lang::En => format!("Move {n}"),
        }
    }

    pub fn wins_by(&self, winner: Color, margin: i32) -> String {
        let side = self.color(winner);
        match self.lang {
            Lang::Ja => format!("{side}の{margin}石勝ち"),
            Lang::En => format!("{side} wins by {margin}"),
        }
    }

    pub fn color(&self, color: Color) -> &'static str {
        match (self.lang, color) {
            (Lang::Ja, Color::Black) => "黒",
            (Lang::Ja, Color::White) => "白",
            (Lang::En, Color::Black) => "Black",
            (Lang::En, Color::White) => "White",
        }
    }

    pub fn preset(&self, preset: Preset) -> &'static str {
        match (self.lang, preset) {
            (Lang::Ja, Preset::Beginner) => "初級",
            (Lang::Ja, Preset::Intermediate) => "中級",
            (Lang::Ja, Preset::Advanced) => "上級",
            (Lang::Ja, Preset::Expert) => "達人",
            (Lang::Ja, Preset::Strongest) => "最強",
            (Lang::En, Preset::Beginner) => "Beginner",
            (Lang::En, Preset::Intermediate) => "Intermediate",
            (Lang::En, Preset::Advanced) => "Advanced",
            (Lang::En, Preset::Expert) => "Expert",
            (Lang::En, Preset::Strongest) => "Strongest",
        }
    }

    pub fn current_position(&self, move_number: usize) -> String {
        match self.lang {
            Lang::Ja => format!("現在の局面（{move_number}手目）"),
            Lang::En => format!("Current position (move {move_number})"),
        }
    }

    pub fn manual_title(&self, color: Color) -> String {
        let side = self.color(color);
        match self.lang {
            Lang::Ja => format!("{side}のマニュアル設定"),
            Lang::En => format!("Manual settings for {side}"),
        }
    }

    /// 先読みの深さ（例: `"12手読み"` / `"12 moves ahead"`）を返す。
    pub fn depth(&self, depth: u8) -> String {
        match self.lang {
            Lang::Ja => format!("{depth}手読み"),
            Lang::En => format!("{depth} moves ahead"),
        }
    }

    pub fn seconds(&self, seconds: u64) -> String {
        match self.lang {
            Lang::Ja => format!("{seconds} 秒"),
            Lang::En => format!("{seconds} s"),
        }
    }

    pub fn logical_cpus(&self, n: usize) -> String {
        match self.lang {
            Lang::Ja => format!("論理CPU {n}"),
            Lang::En => format!("{n} logical CPUs"),
        }
    }

    pub fn moves(&self, n: usize) -> String {
        match self.lang {
            Lang::Ja => format!("{n}手"),
            Lang::En => format!("{n} moves"),
        }
    }

    pub fn exported(&self, path: &str) -> String {
        match self.lang {
            Lang::Ja => format!("書き出しました: {path}"),
            Lang::En => format!("Exported to {path}"),
        }
    }

    pub fn load_failed(&self, error: &str) -> String {
        match self.lang {
            Lang::Ja => format!("開けませんでした: {error}"),
            Lang::En => format!("Could not open: {error}"),
        }
    }

    pub fn save_failed(&self, error: &str) -> String {
        match self.lang {
            Lang::Ja => format!("保存できませんでした: {error}"),
            Lang::En => format!("Could not save: {error}"),
        }
    }

    /// 合法手がなく相手番から対局が始まる場合の案内文を返す。
    pub fn pass_start(&self, side: Color) -> String {
        let current = self.color(side);
        let other = self.color(side.opponent());
        match self.lang {
            Lang::Ja => format!("{current}に打てる手がないため、{other}番から始めます。"),
            Lang::En => format!("{current} cannot move, so {other} starts."),
        }
    }

    /// 盤面上の評価値の下に表示する短い深さ表記（例: `"12手"` / `"d12"`）を返す。
    pub fn depth_mark(&self, depth: u8) -> String {
        match self.lang {
            Lang::Ja => format!("{depth}手"),
            Lang::En => format!("d{depth}"),
        }
    }

    pub fn book_plies(&self, plies: u8) -> String {
        self.moves(usize::from(plies))
    }

    /// Book 生成の進捗行（総局面数・今回の追加数・経過時間）を整形する。
    pub fn book_progress(&self, positions: usize, added: u64, seconds: u64) -> String {
        let (mins, secs) = (seconds / 60, seconds % 60);
        match self.lang {
            Lang::Ja => format!("{positions}局面（+{added}）· {mins}:{secs:02}"),
            Lang::En => format!("{positions} positions (+{added}) · {mins}:{secs:02}"),
        }
    }

    pub fn book_error(&self, error: &str) -> String {
        match self.lang {
            Lang::Ja => format!("生成できませんでした: {error}"),
            Lang::En => format!("Could not grow the book: {error}"),
        }
    }

    pub fn search_revision(&self, revision: u32) -> String {
        match self.lang {
            Lang::Ja => format!("探索リビジョン {revision}"),
            Lang::En => format!("search revision {revision}"),
        }
    }

    /// 評価モデルのフォーマット種別と MPC（ProbCut 枝刈り）較正の有無を説明する文字列を返す。
    /// DPR2 フォーマット v3 以降ではパターン評価に加えて埋め込みネットワークが使用される。
    pub fn model_format(&self, format: u32, calibrated: bool) -> String {
        let (kind, mpc) = match self.lang {
            Lang::Ja => (
                if format >= 3 {
                    "パターン＋埋め込みネットワーク"
                } else {
                    "パターン"
                },
                if calibrated {
                    "MPC較正あり"
                } else {
                    "MPC較正なし"
                },
            ),
            Lang::En => (
                if format >= 3 {
                    "patterns + embedding network"
                } else {
                    "patterns"
                },
                if calibrated {
                    "MPC calibrated"
                } else {
                    "no MPC calibration"
                },
            ),
        };
        format!("DPR2 v{format} · {kind} · {mpc}")
    }

    pub fn book_loaded(&self, positions: usize) -> String {
        match self.lang {
            Lang::Ja => format!("{positions}局面"),
            Lang::En => format!("{positions} positions"),
        }
    }

    pub fn passes(&self, color: Color) -> String {
        let side = self.color(color);
        match self.lang {
            Lang::Ja => format!("{side}はパス"),
            Lang::En => format!("{side} passes"),
        }
    }

    pub fn record_error(&self, move_number: usize) -> String {
        match self.lang {
            Lang::Ja => format!("{move_number}手目が打てません"),
            Lang::En => format!("Move {move_number} cannot be played"),
        }
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
    exact_mark: "完全",
    book: "Book",
    book_group: "Book（定石）",
    book_file: "ファイル",
    book_none: "未選択",
    book_choose: "選択…",
    book_remove: "外す",
    book_use: "AIがBookの手を指す",
    book_note: "AIはBookの最善手を指します。Bookにない局面では通常どおり読みます。",
    book_loading: "読み込み中…",
    book_other_model: "評価モデルが違うため使えません",
    book_failed: "読み込めませんでした",
    version_group: "バージョン情報",
    version_app: "アプリ",
    version_book_lib: "Bookライブラリ",
    version_model: "評価関数",
    version_not_loaded: "まだ読み込んでいません",
    analysis_id: "解析ID",
    book_written_by: "作成",
    book_grown_with: "最後の生成",
    book_model_label: "モデル",
    book_work: "Book作成",
    book_work_file: "作業ファイル",
    book_new: "新規…",
    book_open: "開く…",
    book_from: "開始局面",
    book_from_current: "表示中の局面",
    book_level: "レベル",
    book_max_plies: "最大手数",
    book_plies_note: "開始局面から",
    book_loss: "1手の許容損失",
    book_loss_note: "最善手との石差",
    book_cumulative: "累積損失",
    book_cumulative_note: "手順全体での合計",
    book_no_limit: "なし",
    book_exhausted: "許容損失の範囲で広げられる手がなくなりました。",
    book_memory: "メモリの上限に達しました。",
    book_stopped: "停止しました。",
    book_start: "生成",
    book_stop: "停止",
    book_generating: "Bookを生成中…",
    book_updated: "対局用のBookを更新しました。",
    book_other_work: "別の評価モデルで作られた作業ファイルです。",
    book_moves: "この局面の手",
    book_link: "登録",
    book_unlink: "解除",
    book_not_in: "この局面はまだBookにありません。",
    book_work_note: "停止するまで生成を続けます。生成中はAIと解析が止まります。作業ファイルは10秒ごとに保存し、終わると同じ場所に対局用の .dbook を書き出して使います。",
    book_from_initial: "初期配置",
    book_from_record: "棋譜",
    book_leaf: "リーフ誤差",
    book_leaf_note: "まだリンクしていない手を新しくリンクする時の1手損失",
    book_fix: "Book修正",
    book_fix_note: "Book修正は探索をせず、リンクと読みの記録から評価値を計算し直します。局面は増えません。",
    book_fixing: "Bookを修正中…",
    book_fixed: "修正が終わりました。",
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
    study: "検討",
    studying: "検討中（本譜は残ります）",
    main_line: "本譜",
    study_line: "検討",
    back_to_main: "本譜に戻る",
    exact_solve: "完全読み",
    analyze_record: "全棋譜を解析",
    reanalyze: "再解析",
    stop: "停止",
    analyzing: "解析中",
    candidate_values: "評価値",
    analysis_level: "解析の強さ",
    analysis_heading: "局面解析",
    value_label: "評価値",
    best_move: "最善手",
    expected_line: "読み筋",
    reading_label: "読み",
    candidates: "打てる手の評価",
    graph_title: "評価値の推移",
    black_ahead: "黒有利",
    white_ahead: "白有利",
    start_label: "開始",
    record_heading: "棋譜",
    opening_heading: "定石",
    game_finished: "終局",
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
    theme: "テーマ",
    theme_system: "システム",
    theme_light: "ライト",
    theme_dark: "ダーク",
    flip_animation: "石を返す動き",
    animation_normal: "通常",
    animation_short: "短く",
    animation_off: "なし",
    sound: "効果音",
    on: "オン",
    off: "オフ",
    copy_record: "棋譜をコピー",
    enter_record: "棋譜を入力",
    record_copied: "コピーしました",
    load: "読み込む",
    record_example: "例: f5d6c3d3c4",
    ai_log: "AIログ",
    turn_board: "盤を回転",
    mirror_board: "盤を反転",
    no_ai_moves: "AIはまだ打っていません",
    log_number: "手",
    log_move: "着手",
    log_reading: "読み",
    log_time: "時間",
    log_nodes: "ノード数",
    log_strength: "強さ",
    lang: Lang::Ja,
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
    exact_mark: "exact",
    book: "Book",
    book_group: "Opening book",
    book_file: "File",
    book_none: "None",
    book_choose: "Choose…",
    book_remove: "Remove",
    book_use: "The AI plays book moves",
    book_note: "The AI plays the book's best move. Out of the book, it reads as usual.",
    book_loading: "Loading…",
    book_other_model: "Made with another model, so not used",
    book_failed: "Could not be read",
    version_group: "Version",
    version_app: "App",
    version_book_lib: "Book library",
    version_model: "Evaluation",
    version_not_loaded: "Not loaded yet",
    analysis_id: "Analysis ID",
    book_written_by: "Written by",
    book_grown_with: "Last grown with",
    book_model_label: "Model",
    book_work: "Book builder",
    book_work_file: "Work file",
    book_new: "New…",
    book_open: "Open…",
    book_from: "From",
    book_from_current: "The position shown",
    book_level: "Level",
    book_max_plies: "Longest line",
    book_plies_note: "from the start",
    book_loss: "Loss per move",
    book_loss_note: "discs below the best",
    book_cumulative: "Total loss",
    book_cumulative_note: "along a line",
    book_no_limit: "none",
    book_exhausted: "No more moves within the losses. ",
    book_memory: "The memory limit was reached. ",
    book_stopped: "Stopped. ",
    book_start: "Grow",
    book_stop: "Stop",
    book_generating: "Growing the book…",
    book_updated: "The book for play was updated.",
    book_other_work: "This work file was made with another model.",
    book_moves: "Moves here",
    book_link: "Link",
    book_unlink: "Unlink",
    book_not_in: "Not in the book yet.",
    book_work_note: "It grows until stopped; meanwhile the AI and the analysis wait. The work file is saved every 10 s; at the end a .dbook for play is written next to it and used.",
    book_from_initial: "The start",
    book_from_record: "A record",
    book_leaf: "Leaf loss",
    book_leaf_note: "for a move not yet linked to become a link",
    book_fix: "Fix the book",
    book_fix_note: "Fixing searches nothing: it works the values out again from the links and what was read. No position is added.",
    book_fixing: "Fixing the book…",
    book_fixed: "The fix is done. ",
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
    study: "Study",
    studying: "Studying (the record is kept)",
    main_line: "Record",
    study_line: "Study",
    back_to_main: "Back to the record",
    exact_solve: "Solve exactly",
    analyze_record: "Analyse the record",
    reanalyze: "Analyse again",
    stop: "Stop",
    analyzing: "Analysing",
    candidate_values: "Move values",
    analysis_level: "Analysis level",
    analysis_heading: "Position analysis",
    value_label: "Value",
    best_move: "Best move",
    expected_line: "Expected line",
    reading_label: "Searched",
    candidates: "Every move",
    graph_title: "Evaluation over the game",
    black_ahead: "Black ahead",
    white_ahead: "White ahead",
    start_label: "Start",
    record_heading: "Moves",
    opening_heading: "Opening",
    game_finished: "Game over",
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
    theme: "Theme",
    theme_system: "System",
    theme_light: "Light",
    theme_dark: "Dark",
    flip_animation: "Turning discs",
    animation_normal: "Normal",
    animation_short: "Short",
    animation_off: "Off",
    sound: "Sound",
    on: "On",
    off: "Off",
    copy_record: "Copy record",
    enter_record: "Enter record",
    record_copied: "Copied",
    load: "Load",
    record_example: "e.g. f5d6c3d3c4",
    ai_log: "AI log",
    turn_board: "Turn board",
    mirror_board: "Mirror board",
    no_ai_moves: "The AI has not moved yet",
    log_number: "No.",
    log_move: "Move",
    log_reading: "Search",
    log_time: "Time",
    log_nodes: "Nodes",
    log_strength: "Level",
    lang: Lang::En,
};
