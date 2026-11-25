//! 設定ファイル（`settings.json`）の読み書きを行うモジュール。
//!
//! `#[serde(default)]` を付けることで、知らない項目は無視し、足りない項目には既定値を補うようにしている。
//! これにより、アプリをアップデートしたりダウングレードしたりしても設定ファイルをそのまま読み込める。
use super::atomic_file;
use crate::{
    app::AnalysisSettings,
    domain::{
        BoardView, Preset, Settings,
        player::MOVE_TIMES,
        settings::{
            AiChoice, Animation, BookSettings, GRAPH_HEIGHTS, ManualMode, NewGameForm, SideForm,
            TT_SIZES_MIB, ThemeChoice,
        },
    },
    i18n::Lang,
    services::engine::Resources,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `settings.json` のスキーマバージョン。
const SCHEMA_VERSION: u32 = 1;
/// 設定ファイルの読み込みサイズ上限（64 KiB）。
const MAX_BYTES: u64 = 64 * 1024;

/// `settings.json` v1 形式の JSON ルート構造体。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsV1 {
    /// スキーマバージョン。
    pub schema_version: u32,
    /// 表示言語（`"ja"` または `"en"`）。
    pub language: String,
    /// 合法手マーカーの表示有無。
    pub show_legal_moves: bool,
    /// 探索スレッド数。
    pub threads: usize,
    /// 置換表（計算済み局面のキャッシュ）に割り当てるメモリサイズ（MiB）。
    pub tt_mib: usize,
    /// 解析時の探索レベル。
    ///
    /// 旧バージョンのファイルには `analysis_time_ms` / `batch_time_ms` が含まれる場合があるが、
    /// 現在はレベル指定のみを使用するため無視される。
    pub analysis_level: u8,
    /// 悪手マークの損失石数閾値。
    pub loss_threshold: i32,
    /// 解析中に候補手の評価値を盤面に表示するか。
    pub candidate_values: bool,
    /// テーマ設定（`"system"`, `"light"`, `"dark"`）。
    pub theme: String,
    /// 反転アニメーション設定（`"normal"`, `"short"`, `"off"`）。
    pub animation: String,
    /// 効果音の有効/無効。
    pub sound: bool,
    /// 効果音の音量（0〜100）。
    pub volume: u8,
    /// 前回終了時のウィンドウ幅（論理ピクセル）。
    pub window_width: Option<u32>,
    /// 前回終了時のウィンドウ高さ（論理ピクセル）。
    pub window_height: Option<u32>,
    /// 新規対局ダイアログの前回入力値。
    pub new_game: NewGameV1,
    /// 盤面の時計回り 90 度回転回数と左右反転フラグ。
    pub board_turns: u8,
    /// 盤面の左右反転フラグ。
    pub board_mirrored: bool,
    /// Book（`.dbook`）のファイルパスと使用フラグ。
    pub book_path: Option<String>,
    /// Book の有効/無効。
    pub book_enabled: bool,
    /// 評価値グラフ領域の高さ（論理ピクセル）。
    pub graph_height: u16,
}

/// 新規対局フォームの永続化表現。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NewGameV1 {
    /// 黒番の設定。
    pub black: SideV1,
    /// 白番の設定。
    pub white: SideV1,
}

/// 片側プレイヤー設定の永続化表現。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SideV1 {
    /// AI 担当フラグ。
    pub ai: bool,
    /// プリセット名または `"manual"`。
    pub strength: String,
    /// 手動時のモード（`"level"` または `"time"`）。
    pub manual: String,
    /// 手動時の探索レベル。
    pub level: u8,
    /// 手動時の思考時間制限（ミリ秒）。
    pub time_ms: u64,
}

impl From<SideForm> for SideV1 {
    fn from(side: SideForm) -> Self {
        Self {
            ai: side.ai,
            strength: match side.choice {
                AiChoice::Preset(p) => p.key().into(),
                AiChoice::Manual => "manual".into(),
            },
            manual: match side.mode {
                ManualMode::Level => "level".into(),
                ManualMode::Time => "time".into(),
            },
            level: side.level,
            time_ms: side.time_duration().as_millis() as u64,
        }
    }
}

impl SideV1 {
    /// 不明な値や範囲外の値が含まれていた場合は `default` の該当フィールドへフォールバックする。
    fn to_form(&self, default: SideForm) -> SideForm {
        let choice = if self.strength == "manual" {
            AiChoice::Manual
        } else {
            Preset::from_key(&self.strength).map_or(default.choice, AiChoice::Preset)
        };
        SideForm {
            ai: self.ai,
            choice,
            mode: if self.manual == "time" {
                ManualMode::Time
            } else {
                ManualMode::Level
            },
            level: if (1..=60).contains(&self.level) {
                self.level
            } else {
                default.level
            },
            time: MOVE_TIMES
                .iter()
                .position(|t| t.as_millis() as u64 == self.time_ms)
                .unwrap_or(default.time),
        }
    }
}

impl Default for SideV1 {
    fn default() -> Self {
        SideForm::human().into()
    }
}

impl Default for NewGameV1 {
    fn default() -> Self {
        Self::from(NewGameForm::default())
    }
}

impl From<NewGameForm> for NewGameV1 {
    fn from(form: NewGameForm) -> Self {
        Self {
            black: form.sides[0].into(),
            white: form.sides[1].into(),
        }
    }
}

impl Default for SettingsV1 {
    fn default() -> Self {
        Self::from(Settings::default())
    }
}

impl From<Settings> for SettingsV1 {
    fn from(s: Settings) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            language: match s.lang {
                Lang::Ja => "ja".into(),
                Lang::En => "en".into(),
            },
            show_legal_moves: s.show_legal_moves,
            threads: s.resources.threads,
            tt_mib: s.resources.tt_mib,
            analysis_level: s.analysis.level,
            loss_threshold: s.analysis.loss_threshold,
            candidate_values: s.candidate_values,
            theme: match s.theme {
                ThemeChoice::System => "system",
                ThemeChoice::Light => "light",
                ThemeChoice::Dark => "dark",
            }
            .into(),
            animation: match s.animation {
                Animation::Normal => "normal",
                Animation::Short => "short",
                Animation::Off => "off",
            }
            .into(),
            sound: s.sound,
            volume: s.volume,
            window_width: s.window_size.map(|(w, _)| w),
            window_height: s.window_size.map(|(_, h)| h),
            new_game: s.new_game.into(),
            board_turns: s.board_view.turns(),
            board_mirrored: s.board_view.mirrored,
            book_path: s.book.path.as_ref().map(|p| p.display().to_string()),
            book_enabled: s.book.enabled,
            graph_height: s.graph_height,
        }
    }
}

impl SettingsV1 {
    /// 範囲外の値をクランプまたは既定値へフォールバックさせて [`Settings`] に変換する。
    pub fn to_settings(&self) -> Settings {
        let default = Settings::default();
        let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
        Settings {
            lang: if self.language == "en" {
                Lang::En
            } else {
                Lang::Ja
            },
            show_legal_moves: self.show_legal_moves,
            resources: Resources {
                threads: if (1..=cpus).contains(&self.threads) {
                    self.threads
                } else {
                    default.resources.threads
                },
                tt_mib: if TT_SIZES_MIB.contains(&self.tt_mib) {
                    self.tt_mib
                } else {
                    default.resources.tt_mib
                },
            },
            analysis: AnalysisSettings {
                level: if (1..=60).contains(&self.analysis_level) {
                    self.analysis_level
                } else {
                    default.analysis.level
                },
                loss_threshold: self.loss_threshold.clamp(1, 64),
            },
            candidate_values: self.candidate_values,
            theme: match self.theme.as_str() {
                "light" => ThemeChoice::Light,
                "dark" => ThemeChoice::Dark,
                _ => ThemeChoice::System,
            },
            animation: match self.animation.as_str() {
                "short" => Animation::Short,
                "off" => Animation::Off,
                _ => Animation::Normal,
            },
            sound: self.sound,
            volume: self.volume.min(100),
            // 画面に収まらない異常なウィンドウサイズは破棄する。
            window_size: self
                .window_width
                .zip(self.window_height)
                .filter(|&(w, h)| (800..=10_000).contains(&w) && (560..=10_000).contains(&h)),
            new_game: NewGameForm {
                sides: [
                    self.new_game.black.to_form(default.new_game.sides[0]),
                    self.new_game.white.to_form(default.new_game.sides[1]),
                ],
                from_current: false,
            },
            board_view: BoardView::new(self.board_turns, self.board_mirrored),
            book: BookSettings {
                path: self.book_path.as_ref().map(PathBuf::from),
                enabled: self.book_enabled,
            },
            graph_height: if GRAPH_HEIGHTS.contains(&self.graph_height) {
                self.graph_height
            } else {
                default.graph_height
            },
        }
    }

    /// 整形済み JSON 文字列へシリアライズする。
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("settings always serialize")
    }
}

/// 設定ファイルを読み込む。
///
/// ファイルが存在しない場合や、本体・バックアップともに破損していた場合は
/// 既定値（[`Settings::default`]）を返し、アプリが必ず起動できるようにする。
pub fn load(path: &Path) -> Settings {
    if !path.exists() && !atomic_file::backup_path(path).exists() {
        return Settings::default();
    }
    match atomic_file::read_with_backup(path, MAX_BYTES, |text: &str| {
        atomic_file::ParseOutcome::from(serde_json::from_str::<SettingsV1>(text))
    }) {
        Ok(file) => file.to_settings(),
        Err(error) => {
            eprintln!("[settings] {}: {error}; using defaults", path.display());
            Settings::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::atomic_file::tests::temp_dir;
    use std::fs;

    #[test]
    fn settings_round_trip_and_survive_bad_files() {
        let dir = temp_dir("settings");
        let path = dir.join("settings.json");
        assert_eq!(load(&path), Settings::default());

        let defaults = Settings::default();
        let mut new_game = defaults.new_game;
        new_game.sides[0] = SideForm {
            ai: true,
            choice: AiChoice::Manual,
            mode: ManualMode::Time,
            time: 4,
            ..new_game.sides[0]
        };
        let settings = Settings {
            lang: Lang::En,
            show_legal_moves: false,
            new_game,
            book: BookSettings {
                path: Some(PathBuf::from("openings.dbook")),
                enabled: false,
            },
            ..defaults
        };
        atomic_file::write(
            &path,
            SettingsV1::from(settings.clone()).to_json().as_bytes(),
        )
        .unwrap();
        assert_eq!(load(&path), settings);

        // Unknown and missing fields; values out of range.
        fs::write(&path, r#"{"language":"en","future":1,"tt_mib":3}"#).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.lang, Lang::En);
        assert_eq!(
            loaded.resources.tt_mib,
            Settings::default().resources.tt_mib
        );

        fs::write(&path, "not json").unwrap();
        let _ = fs::remove_file(atomic_file::backup_path(&path));
        assert_eq!(load(&path), Settings::default());
        let _ = fs::remove_dir_all(dir);
    }
}
