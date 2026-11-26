//! 設定画面で変更されるアプリケーション設定と、新規対局ダイアログの前回入力値を保持するモジュール。
use super::{Player, Preset, Strength, player::MOVE_TIMES};
use crate::{engine_loader, i18n::Lang};
use deft_reversi_engine::Square;
use std::{path::PathBuf, time::Duration};

/// 探索エンジンに割り当てる計算リソース。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resources {
    /// 探索に使う CPU スレッド数。
    pub threads: usize,
    /// 置換表（計算済み局面のキャッシュ）に割り当てるメモリ容量（MiB）。
    pub tt_mib: usize,
}

/// 局面解析モードの設定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisSettings {
    /// 表示中局面の全合法手評価および棋譜全体解析で使用する探索レベル。
    pub level: u8,
    /// この石数以上の損失がある手を棋譜上で悪手としてマークする閾値。
    pub loss_threshold: i32,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            level: 20,
            loss_threshold: 3,
        }
    }
}

/// アプリケーション全体の設定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// 表示言語。
    pub lang: Lang,
    /// 盤面上に合法手マーカーを表示するか。
    pub show_legal_moves: bool,
    /// エンジンのスレッド数・置換表サイズ設定。
    pub resources: Resources,
    /// 解析モードの探索レベル・悪手閾値設定。
    pub analysis: AnalysisSettings,
    /// 解析モード中に合法手上に評価値（またはドット）を表示するか。
    pub candidate_values: bool,
    /// UI テーマ設定。
    pub theme: ThemeChoice,
    /// 石の反転アニメーション速度設定。
    pub animation: Animation,
    /// 着手音などの効果音を有効にするか。
    pub sound: bool,
    /// 効果音の音量（0〜100）。
    pub volume: u8,
    /// 前回終了時のウィンドウサイズ（論理ピクセル）。
    pub window_size: Option<(u32, u32)>,
    /// 次回ダイアログを開いたときに引き継ぐ新規対局フォームの状態。
    pub new_game: NewGameForm,
    /// 盤面の回転・左右反転状態。
    pub board_view: BoardView,
    /// AI が参照する Book（`.dbook`）設定。
    pub book: BookSettings,
    /// 評価値グラフ領域の高さ（論理ピクセル、ドラッグでリサイズ可能）。
    pub graph_height: u16,
}

/// 評価値グラフのドラッグ可能な高さ範囲（論理ピクセル）。
pub const GRAPH_HEIGHTS: std::ops::RangeInclusive<u16> = 60..=400;

/// AI 対局で使用する Book（`.dbook`）の設定。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BookSettings {
    /// 選択された `.dbook` ファイルパス（未選択時は `None`）。
    pub path: Option<PathBuf>,
    /// 無効化時はファイル選択を維持したままブック参照をスキップする。
    pub enabled: bool,
}

impl BookSettings {
    /// AI の着手に Book を利用すべきかどうかを返す。
    pub fn use_for_ai(&self) -> bool {
        self.enabled && self.path.is_some()
    }
}

/// 画面上の盤面表示向き（左右反転および時計回り 90 度単位の回転）。
///
/// 「左右反転してから時計回りに回転する」という順序で固定しており、
/// 画面座標と盤面座標の相互変換（[`BoardView::square_at`] と [`BoardView::place_of`]）が
/// 常に互いに逆変換になるようにしている。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardView {
    /// 時計回りの 90 度回転回数（`0..4`）。
    turns: u8,
    /// 左右反転の有無。
    pub mirrored: bool,
}

impl BoardView {
    /// 回転回数と左右反転フラグから [`BoardView`] を生成する。
    pub fn new(turns: u8, mirrored: bool) -> Self {
        Self {
            turns: turns % 4,
            mirrored,
        }
    }

    /// 時計回りの 90 度回転回数（`0..4`）を返す。
    pub fn turns(self) -> u8 {
        self.turns
    }

    /// 時計回りに 90 度回転した新しい [`BoardView`] を返す。
    pub fn turned(self) -> Self {
        Self {
            turns: (self.turns + 1) % 4,
            ..self
        }
    }

    /// 左右反転を切り替えた新しい [`BoardView`] を返す。
    pub fn mirrored(self) -> Self {
        Self {
            mirrored: !self.mirrored,
            ..self
        }
    }

    /// 画面上の列 `x`・行 `y`（左上原点 `0..8`）に対応する盤面上の [`Square`] を返す。
    pub fn square_at(self, x: u8, y: u8) -> Square {
        debug_assert!(x < 8 && y < 8);
        let (mut x, mut y) = (x, y);
        // 時計回り 90 度回転は盤面 (x, y) を画面 (7 - y, x) に写すため、その逆変換を適用する。
        for _ in 0..self.turns % 4 {
            (x, y) = (y, 7 - x);
        }
        if self.mirrored {
            x = 7 - x;
        }
        Square::from_xy(x, y).expect("x and y are 0..8")
    }

    /// 盤面上の [`Square`] が画面上のどの列・行（左上原点 `0..8`）に表示されるかを返す。
    pub fn place_of(self, square: Square) -> (u8, u8) {
        let (mut x, mut y) = (square.x(), square.y());
        if self.mirrored {
            x = 7 - x;
        }
        for _ in 0..self.turns % 4 {
            (x, y) = (7 - y, x);
        }
        (x, y)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: Lang::Ja,
            show_legal_moves: true,
            resources: engine_loader::default_resources(),
            analysis: AnalysisSettings::default(),
            candidate_values: true,
            theme: ThemeChoice::System,
            animation: Animation::Normal,
            sound: true,
            volume: 60,
            window_size: None,
            new_game: NewGameForm::default(),
            board_view: BoardView::default(),
            book: BookSettings {
                enabled: true,
                ..BookSettings::default()
            },
            graph_height: 80,
        }
    }
}

/// アプリケーションのテーマ選択。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    /// OS の外観設定に追従する。
    System,
    /// ライトテーマ。
    Light,
    /// ダークテーマ。
    Dark,
}

/// 石の反転アニメーション速度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Animation {
    /// 標準速度。
    Normal,
    /// 半分の時間（高速）。
    Short,
    /// アニメーションなし。
    Off,
}

/// 設定画面で選択可能な置換表サイズ候補（MiB）。
pub const TT_SIZES_MIB: [usize; 6] = [32, 64, 128, 256, 512, 1024];

/// 新規対局ダイアログでの AI 強さ指定方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiChoice {
    /// プリセットを使用。
    Preset(Preset),
    /// 手動（レベルまたは思考時間）で指定。
    Manual,
}

/// 手動指定時のモード。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualMode {
    /// 探索レベル指定。
    Level,
    /// 1 手あたりの思考時間指定。
    Time,
}

/// 新規対局ダイアログにおける片側（黒または白）の設定フォーム状態。
///
/// プリセット選択中も手動設定値（`level` / `time`）を保持し、手動に戻したときに復元できるようにする。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SideForm {
    /// AI が担当する場合は `true`、人間なら `false`。
    pub ai: bool,
    /// プリセットか手動かの選択。
    pub choice: AiChoice,
    /// 手動時のモード（レベル / 思考時間）。
    pub mode: ManualMode,
    /// 手動時の探索レベル。
    pub level: u8,
    /// 手動時の思考時間（`MOVE_TIMES` 配列のインデックス）。
    pub time: usize,
}

impl SideForm {
    /// 人間プレイヤーを既定値としたフォーム状態を返す。
    pub fn human() -> Self {
        Self {
            ai: false,
            choice: AiChoice::Preset(Preset::Intermediate),
            mode: ManualMode::Level,
            level: 20,
            time: 2,
        }
    }

    /// 選択されている思考時間インデックスに対応する [`Duration`] を返す。
    pub fn time_duration(&self) -> Duration {
        MOVE_TIMES[self.time.min(MOVE_TIMES.len() - 1)]
    }

    /// 現在のフォーム設定から [`Strength`] を構築する。
    pub fn strength(&self) -> Strength {
        match (self.choice, self.mode) {
            (AiChoice::Preset(preset), _) => Strength::Preset(preset),
            (AiChoice::Manual, ManualMode::Level) => Strength::Level(self.level),
            (AiChoice::Manual, ManualMode::Time) => Strength::TimePerMove(self.time_duration()),
        }
    }

    /// 現在のフォーム設定から [`Player`] を構築する。
    pub fn player(&self) -> Player {
        if self.ai {
            Player::Ai(self.strength())
        } else {
            Player::Human
        }
    }
}

/// 新規対局ダイアログ全体の入力状態。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewGameForm {
    /// 黒番（インデックス 0）・白番（インデックス 1）の設定。
    pub sides: [SideForm; 2],
    /// 現在の局面から対局を開始する場合は `true`、初期局面からなら `false`。
    pub from_current: bool,
}

impl Default for NewGameForm {
    fn default() -> Self {
        Self {
            sides: [
                SideForm::human(),
                SideForm {
                    ai: true,
                    ..SideForm::human()
                },
            ],
            from_current: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(view: BoardView, x: u8, y: u8) -> String {
        view.square_at(x, y).to_string().to_lowercase()
    }

    #[test]
    fn turns_and_mirrors_move_the_corners() {
        let plain = BoardView::default();
        assert_eq!(named(plain, 0, 0), "a1");
        // A quarter turn clockwise brings a8 to the top left.
        assert_eq!(named(plain.turned(), 0, 0), "a8");
        assert_eq!(named(plain.turned().turned(), 0, 0), "h8");
        assert_eq!(named(plain.mirrored(), 0, 0), "h1");
        assert_eq!(plain.turned().turned().turned().turned(), plain);
        for view in [plain.turned(), plain.mirrored().turned()] {
            for square in (0..64).map(|i| view.square_at(i % 8, i / 8)) {
                let (x, y) = view.place_of(square);
                assert_eq!(view.square_at(x, y), square);
            }
        }
    }
}
