//! `settings.json`. Unknown fields are ignored and missing ones take their
//! defaults, so older and newer versions read each other's files.
use super::atomic_file;
use crate::{
    app::AnalysisSettings,
    domain::{
        Preset, Settings,
        player::MOVE_TIMES,
        settings::{
            AiChoice, Animation, ManualMode, NewGameForm, SideForm, TT_SIZES_MIB, ThemeChoice,
        },
    },
    i18n::Lang,
    services::engine::Resources,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA_VERSION: u32 = 1;
const MAX_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsV1 {
    pub schema_version: u32,
    pub language: String,
    pub show_legal_moves: bool,
    pub threads: usize,
    pub tt_mib: usize,
    pub analysis_level: u8,
    /// Older files also carry `analysis_time_ms` and `batch_time_ms`, which
    /// are ignored: analysis is set by level only.
    pub loss_threshold: i32,
    pub candidate_values: bool,
    /// "system", "light" or "dark".
    pub theme: String,
    /// "normal", "short" or "off".
    pub animation: String,
    pub sound: bool,
    pub volume: u8,
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
    pub new_game: NewGameV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NewGameV1 {
    pub black: SideV1,
    pub white: SideV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SideV1 {
    pub ai: bool,
    /// A preset's name, or "manual".
    pub strength: String,
    /// "level" or "time".
    pub manual: String,
    pub level: u8,
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
            time_ms: MOVE_TIMES[side.time].as_millis() as u64,
        }
    }
}

impl SideV1 {
    /// Anything unknown falls back to the default side.
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
        Self::from(Settings::default().new_game)
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
        }
    }
}

impl SettingsV1 {
    /// Values out of range take their defaults.
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
            // A size the screen cannot have is dropped.
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
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("settings always serialize")
    }
}

/// A missing file gives the defaults; a broken one is set aside and also
/// gives the defaults, so the app always starts.
pub fn load(path: &Path) -> Settings {
    if !path.exists() && !atomic_file::backup_path(path).exists() {
        return Settings::default();
    }
    match atomic_file::read_with_backup(path, MAX_BYTES, |text: &str| {
        serde_json::from_str::<SettingsV1>(text)
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
            ..defaults
        };
        atomic_file::write(&path, SettingsV1::from(settings).to_json().as_bytes()).unwrap();
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
