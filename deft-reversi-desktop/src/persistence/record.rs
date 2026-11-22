//! The game record file v1 (`.dreversi`, UTF-8 JSON; §8.1 of the plan).
//! The initial position and the actions are the record; status and outcome
//! are written for other readers and recomputed on load.
use crate::domain::{GameInfo, History, Player, Preset, Strength, game::now_rfc3339};
use deft_reversi_engine::{Action, Color, Position};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const FORMAT: &str = "deft-reversi-record";
pub const SCHEMA_VERSION: u32 = 1;
/// Larger files are not read.
pub const MAX_BYTES: u64 = 1024 * 1024;
/// A game has at most 60 moves and as many passes; this leaves room for
/// edited starts and still bounds the replay.
const MAX_ACTIONS: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordV1 {
    pub format: String,
    pub schema_version: u32,
    pub game_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub initial_position: String,
    pub actions: Vec<String>,
    pub cursor: usize,
    pub action_groups: Vec<usize>,
    pub players: PlayersV1,
    pub status: String,
    pub outcome: Option<OutcomeV1>,
    pub engine: EngineInfoV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayersV1 {
    pub black: PlayerV1,
    pub white: PlayerV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlayerV1 {
    Human,
    Ai {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        level: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time_limit_ms: Option<u64>,
    },
}

/// Disc counts with the empty squares given to the winner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeV1 {
    pub black: u32,
    pub white: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineInfoV1 {
    pub version: String,
    pub model: String,
}

/// A record checked and turned back into game state.
#[derive(Clone, Debug)]
pub struct LoadedGame {
    pub game: GameInfo,
    pub players: [Player; 2],
    pub history: History,
}

/// A validated record together with the metadata needed for the game list.
#[derive(Clone, Debug)]
pub struct ParsedRecord {
    pub loaded: LoadedGame,
    pub updated_at: String,
    pub outcome: Option<(u32, u32)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    TooLarge,
    NotJson(String),
    NotARecord,
    /// Written by a newer version of the app.
    NewerVersion(u32),
    Invalid(&'static str),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => write!(f, "the file is larger than {MAX_BYTES} bytes"),
            Self::NotJson(e) => write!(f, "not a valid file: {e}"),
            Self::NotARecord => f.write_str("not a deft-reversi record"),
            Self::NewerVersion(v) => write!(f, "made by a newer version (schema {v})"),
            Self::Invalid(e) => write!(f, "a broken record: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<Player> for PlayerV1 {
    fn from(player: Player) -> Self {
        let ai = |preset: Option<Preset>, level, time_limit_ms| Self::Ai {
            preset: preset.map(|p| p.key().to_string()),
            level,
            time_limit_ms,
        };
        match player {
            Player::Human => Self::Human,
            // The level is also written so other readers know the strength.
            Player::Ai(Strength::Preset(p)) => ai(Some(p), Some(p.level()), None),
            Player::Ai(Strength::Level(level)) => ai(None, Some(level), None),
            Player::Ai(Strength::TimePerMove(limit)) => {
                ai(None, None, Some(limit.as_millis() as u64))
            }
        }
    }
}

impl TryFrom<&PlayerV1> for Player {
    type Error = LoadError;

    fn try_from(player: &PlayerV1) -> Result<Self, LoadError> {
        let PlayerV1::Ai {
            preset,
            level,
            time_limit_ms,
        } = player
        else {
            return Ok(Self::Human);
        };
        let preset = preset.as_deref().and_then(Preset::from_key);
        let strength = match (preset, level, time_limit_ms) {
            (Some(p), _, _) => Strength::Preset(p),
            (None, _, Some(ms)) if (1..=3_600_000).contains(ms) => {
                Strength::TimePerMove(Duration::from_millis(*ms))
            }
            (None, Some(level), _) if (1..=60).contains(level) => Strength::Level(*level),
            _ => return Err(LoadError::Invalid("an AI without a strength")),
        };
        Ok(Self::Ai(strength))
    }
}

/// The engine's position text uses upper-case squares; the record uses
/// lower case, as the UI does.
fn action_text(action: Action) -> String {
    action.to_string().to_lowercase()
}

impl RecordV1 {
    pub fn new(
        game: &GameInfo,
        players: [Player; 2],
        history: &History,
        engine: EngineInfoV1,
    ) -> Self {
        let last = history.position(history.actions().len());
        let finished = last.is_terminal();
        let outcome = finished.then(|| {
            let margin = last.final_score(Color::Black);
            let empties = u32::from(last.empty_count());
            let (mut black, mut white) =
                (last.disc_count(Color::Black), last.disc_count(Color::White));
            if margin > 0 {
                black += empties;
            } else if margin < 0 {
                white += empties;
            }
            OutcomeV1 { black, white }
        });
        Self {
            format: FORMAT.into(),
            schema_version: SCHEMA_VERSION,
            game_id: game.id.clone(),
            created_at: game.created_at.clone(),
            updated_at: now_rfc3339(),
            initial_position: history.position(0).to_string(),
            actions: history.actions().iter().map(|&a| action_text(a)).collect(),
            cursor: history.cursor(),
            action_groups: history.groups().to_vec(),
            players: PlayersV1 {
                black: players[0].into(),
                white: players[1].into(),
            },
            status: if finished { "finished" } else { "playing" }.into(),
            outcome,
            engine,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a record always serializes")
    }

    /// Checks everything before any state changes.
    pub fn parse(text: &str) -> Result<LoadedGame, LoadError> {
        Self::parse_with_meta(text).map(|parsed| parsed.loaded)
    }

    /// Parses and validates a record, also returning `updated_at` and `outcome`
    /// so callers such as the game list do not parse the JSON twice.
    pub fn parse_with_meta(text: &str) -> Result<ParsedRecord, LoadError> {
        if text.len() as u64 > MAX_BYTES {
            return Err(LoadError::TooLarge);
        }
        // The header first, so a newer schema is reported as such even when
        // its fields no longer match this version.
        #[derive(Deserialize)]
        struct Header {
            format: String,
            schema_version: u32,
        }
        let header: Header =
            serde_json::from_str(text).map_err(|e| LoadError::NotJson(e.to_string()))?;
        if header.format != FORMAT {
            return Err(LoadError::NotARecord);
        }
        if header.schema_version > SCHEMA_VERSION {
            return Err(LoadError::NewerVersion(header.schema_version));
        }
        if header.schema_version < SCHEMA_VERSION {
            return Err(LoadError::Invalid("unsupported schema version"));
        }
        let record: Self =
            serde_json::from_str(text).map_err(|e| LoadError::NotJson(e.to_string()))?;
        record.load()
    }

    fn load(self) -> Result<ParsedRecord, LoadError> {
        if !GameInfo::valid_id(&self.game_id) {
            return Err(LoadError::Invalid("a bad game id"));
        }
        if self.actions.len() > MAX_ACTIONS {
            return Err(LoadError::Invalid("too many actions"));
        }
        let initial: Position = self
            .initial_position
            .parse()
            .map_err(|_| LoadError::Invalid("a bad initial position"))?;
        let actions = self
            .actions
            .iter()
            .map(|a| a.parse::<Action>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| LoadError::Invalid("a bad action"))?;
        let history = History::restore(initial, actions, self.action_groups, self.cursor)
            .map_err(LoadError::Invalid)?;
        let loaded = LoadedGame {
            game: GameInfo {
                id: self.game_id,
                created_at: self.created_at,
            },
            players: [
                Player::try_from(&self.players.black)?,
                Player::try_from(&self.players.white)?,
            ],
            history,
        };
        let outcome = self.outcome.map(|o| (o.black, o.white));
        Ok(ParsedRecord {
            loaded,
            updated_at: self.updated_at,
            outcome,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Grouping;
    use deft_reversi_engine::Square;

    fn place(name: &str) -> Action {
        Action::Place(name.parse::<Square>().unwrap())
    }

    fn engine() -> EngineInfoV1 {
        EngineInfoV1 {
            version: "test".into(),
            model: "test".into(),
        }
    }

    fn round_trip(players: [Player; 2], history: &History) -> LoadedGame {
        let game = GameInfo::new();
        let json = RecordV1::new(&game, players, history, engine()).to_json();
        let loaded = RecordV1::parse(&json).unwrap();
        assert_eq!(loaded.game, game);
        assert_eq!(loaded.players, players);
        assert_eq!(loaded.history.actions(), history.actions());
        assert_eq!(loaded.history.groups(), history.groups());
        assert_eq!(loaded.history.cursor(), history.cursor());
        assert_eq!(loaded.history.current(), history.current());
        loaded
    }

    #[test]
    fn round_trips_with_undo_passes_and_an_edited_start() {
        let players = [
            Player::Human,
            Player::Ai(Strength::TimePerMove(Duration::from_secs(5))),
        ];
        let mut history = History::new(Position::initial());
        history.play(place("f5"), Grouping::NewGroup).unwrap();
        history.play(place("d6"), Grouping::Join).unwrap();
        history.play(place("c3"), Grouping::NewGroup).unwrap();
        history.undo();
        round_trip(players, &history);

        // White to move cannot; the start records a pass.
        let bits = |names: &[&str]| {
            names
                .iter()
                .fold(0, |acc, n| acc | n.parse::<Square>().unwrap().bit())
        };
        let start =
            Position::from_colors(bits(&["a1", "d1"]), bits(&["b1", "a2"]), Color::White).unwrap();
        let mut history = History::new(start);
        history.play(place("c1"), Grouping::NewGroup).unwrap();
        round_trip(
            [
                Player::Ai(Strength::Preset(Preset::Strongest)),
                Player::Ai(Strength::Level(33)),
            ],
            &history,
        );
    }

    #[test]
    fn finished_games_record_the_outcome() {
        let bits = |n: &str| n.parse::<Square>().unwrap().bit();
        let start = Position::from_colors(bits("d4"), bits("e4"), Color::Black).unwrap();
        let mut history = History::new(start);
        history.play(place("f4"), Grouping::NewGroup).unwrap();
        let record = RecordV1::new(&GameInfo::new(), [Player::Human; 2], &history, engine());
        assert_eq!(record.status, "finished");
        assert_eq!(
            record.outcome,
            Some(OutcomeV1 {
                black: 64,
                white: 0
            })
        );
    }

    #[test]
    fn bad_files_are_rejected() {
        let history = History::new(Position::initial());
        let good = RecordV1::new(&GameInfo::new(), [Player::Human; 2], &history, engine());
        let with = |change: fn(&mut serde_json::Value)| {
            let mut value = serde_json::to_value(&good).unwrap();
            change(&mut value);
            RecordV1::parse(&value.to_string()).unwrap_err()
        };
        assert!(matches!(RecordV1::parse("{"), Err(LoadError::NotJson(_))));
        assert_eq!(with(|v| v["format"] = "x".into()), LoadError::NotARecord);
        // A newer version is named even when its fields changed.
        assert_eq!(
            with(|v| {
                v["schema_version"] = 2.into();
                v["actions"] = 7.into();
            }),
            LoadError::NewerVersion(2)
        );
        assert!(matches!(
            with(|v| v["schema_version"] = 0.into()),
            LoadError::Invalid(_)
        ));
        assert!(matches!(
            with(|v| v["actions"] = serde_json::json!(["a1"])),
            LoadError::Invalid(_)
        ));
        assert!(matches!(
            with(|v| v["actions"] = serde_json::json!(["pass"])),
            LoadError::Invalid(_)
        ));
        assert!(matches!(
            with(|v| v["cursor"] = 3.into()),
            LoadError::Invalid(_)
        ));
        assert!(matches!(
            with(|v| v["game_id"] = "../../x".into()),
            LoadError::Invalid(_)
        ));
        assert!(matches!(
            with(|v| v["players"]["white"] = serde_json::json!({"kind": "ai"})),
            LoadError::Invalid(_)
        ));
        let huge = " ".repeat(MAX_BYTES as usize + 1);
        assert_eq!(RecordV1::parse(&huge).unwrap_err(), LoadError::TooLarge);
    }
}
