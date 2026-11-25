//! 対局棋譜ファイル v1（`.dreversi`、UTF-8 JSON）の書き出しと検証付き読み込みを行うモジュール。
//!
//! このファイルでは「初期局面」と「着手列」を正（一次情報）として保存し、
//! `status` や `outcome` は外部ツールが中身を覗き見やすいよう参考情報として併記している。
//! 読み込み時には保存された着手列を 1 手ずつ実際に盤面へ並べて（リプレイして）、
//! 非合法手や矛盾がないかを検証した上でアプリの状態へ復元する。
use crate::domain::{GameInfo, History, Player, Preset, Strength, game::now_rfc3339};
use deft_reversi_engine::{Action, Color, Position};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 棋譜ファイルのフォーマット識別子。
pub const FORMAT: &str = "deft-reversi-record";
/// サポートしている棋譜スキーマバージョン。
pub const SCHEMA_VERSION: u32 = 1;
/// 読み込みを許可する棋譜ファイルの最大サイズ（1 MiB）。
pub const MAX_BYTES: u64 = 1024 * 1024;
/// 1 局あたりの最大アクション数上限（60 手＋パス上限に編集開始局面の余裕を加えた値）。
const MAX_ACTIONS: usize = 200;

/// `.dreversi` v1 形式の JSON ルート構造体。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordV1 {
    /// フォーマット識別子（[`FORMAT`]）。
    pub format: String,
    /// スキーマバージョン（[`SCHEMA_VERSION`]）。
    pub schema_version: u32,
    /// 対局 ID。
    pub game_id: String,
    /// 対局開始日時（RFC 3339）。
    pub created_at: String,
    /// 最終更新日時（RFC 3339）。
    pub updated_at: String,
    /// 初期局面の文字列表現。
    pub initial_position: String,
    /// 着手・パス列（小文字表記）。
    pub actions: Vec<String>,
    /// 現在のカーソル位置。
    pub cursor: usize,
    /// Undo グループの開始インデックス列。
    pub action_groups: Vec<usize>,
    /// 黒番・白番のプレイヤー情報。
    pub players: PlayersV1,
    /// 対局状態（`"playing"` または `"finished"`）。
    pub status: String,
    /// 終局時の結果（空きマスを勝者に加算した石数）。
    pub outcome: Option<OutcomeV1>,
    /// 記録時のエンジン・モデル情報。
    pub engine: EngineInfoV1,
}

/// 黒番・白番のプレイヤー定義。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayersV1 {
    /// 黒番プレイヤー。
    pub black: PlayerV1,
    /// 白番プレイヤー。
    pub white: PlayerV1,
}

/// 棋譜ファイル内のプレイヤー表現。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlayerV1 {
    /// 人間。
    Human,
    /// AI（プリセット名・レベル・思考時間制限のいずれかで強さを記録）。
    Ai {
        /// プリセットキー名。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preset: Option<String>,
        /// 探索レベル。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        level: Option<u8>,
        /// 1 手あたりの思考時間上限（ミリ秒）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time_limit_ms: Option<u64>,
    },
}

/// 終局時の最終石数（空きマスを勝者に加算したリバーシ公式ルール準拠のスコア）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeV1 {
    /// 黒の最終石数。
    pub black: u32,
    /// 白の最終石数。
    pub white: u32,
}

/// 対局時に使用したエンジン・評価モデルのバージョン情報。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineInfoV1 {
    /// アプリ／エンジンバージョン。
    pub version: String,
    /// 評価モデル名。
    pub model: String,
}

/// 検証を経てアプリ内の対局状態へ復元された棋譜データ。
#[derive(Clone, Debug)]
pub struct LoadedGame {
    /// 対局 ID と開始日時。
    pub game: GameInfo,
    /// 黒番・白番のプレイヤー。
    pub players: [Player; 2],
    /// 検証済みの棋譜履歴。
    pub history: History,
}

/// 検証済み棋譜と対局一覧表示用メタデータのセット。
///
/// 一覧構築時に JSON を二重パースせずに済むよう `updated_at` と `outcome` も合わせて返す。
#[derive(Clone, Debug)]
pub struct ParsedRecord {
    /// 復元された対局状態。
    pub loaded: LoadedGame,
    /// 最終更新日時（RFC 3339）。
    pub updated_at: String,
    /// 終局済みの場合の `(黒石数, 白石数)`。
    pub outcome: Option<(u32, u32)>,
}

/// 棋譜ファイルの読み込み・検証エラー。
#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    /// ファイルサイズが [`MAX_BYTES`] を超えている。
    TooLarge,
    /// JSON として不正。
    NotJson(String),
    /// `deft-reversi` の棋譜フォーマットではない。
    NotARecord,
    /// より新しいバージョンのアプリで作成されたスキーマ。
    NewerVersion(u32),
    /// スキーマは一致するが内容（ID・局面・手順等）が不正。
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
            // 外部リーダーでも強さを判別できるよう、プリセット時も対応レベルを併記する。
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

/// エンジン内部表記は大文字座標だが、棋譜ファイルでは UI に合わせて小文字で出力する。
fn action_text(action: Action) -> String {
    action.to_string().to_lowercase()
}

impl RecordV1 {
    /// 現在の対局状態から保存用の [`RecordV1`] を構築する。
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

    /// 整形済み JSON 文字列へシリアライズする。
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a record always serializes")
    }

    /// JSON テキストを検証・パースして [`LoadedGame`] を返す。
    pub fn parse(text: &str) -> Result<LoadedGame, LoadError> {
        Self::parse_with_meta(text).map(|parsed| parsed.loaded)
    }

    /// JSON テキストを検証・パースし、対局一覧用のメタデータと合わせて [`ParsedRecord`] を返す。
    pub fn parse_with_meta(text: &str) -> Result<ParsedRecord, LoadError> {
        if text.len() as u64 > MAX_BYTES {
            return Err(LoadError::TooLarge);
        }
        // 新しいスキーマで他フィールドが変更されていても正しく NewerVersion を返せるよう、
        // まずフォーマット識別子とスキーマバージョンだけを先行して読み取る。
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
