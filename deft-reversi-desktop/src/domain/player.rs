//! 各手番のプレイヤー種別（人間 / AI）と AI の強さ設定を定義するモジュール。
use std::{ops::RangeInclusive, time::Duration};

/// エンジンで指定可能な探索レベルの範囲。
pub const LEVEL_RANGE: RangeInclusive<u8> = 1..=60;

/// 手番を担当するプレイヤー種別。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Player {
    /// 人間プレイヤー。
    Human,
    /// 指定した強さで思考する AI プレイヤー。
    Ai(Strength),
}

impl Player {
    /// このプレイヤーが AI であるかどうかを返す。
    pub fn is_ai(self) -> bool {
        matches!(self, Self::Ai(_))
    }
}

/// AI の強さプリセット。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// 初級。
    Beginner,
    /// 中級。
    Intermediate,
    /// 上級。
    Advanced,
    /// 達人。
    Expert,
    /// 最強。
    Strongest,
}

impl Preset {
    /// すべてのプリセットを弱い順に並べた配列。
    pub const ALL: [Self; 5] = [
        Self::Beginner,
        Self::Intermediate,
        Self::Advanced,
        Self::Expert,
        Self::Strongest,
    ];

    /// 設定ファイル・棋譜ファイルへ保存する際のキー文字列。
    pub fn key(self) -> &'static str {
        match self {
            Self::Beginner => "beginner",
            Self::Intermediate => "intermediate",
            Self::Advanced => "advanced",
            Self::Expert => "expert",
            Self::Strongest => "strongest",
        }
    }

    /// キー文字列から対応する [`Preset`] を復元する。
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }

    /// プリセットに対応するエンジン探索レベルを返す。
    ///
    /// レベル 1〜10 では ProbCut（確率的な枝刈り）を使わずに素直に読み、
    /// `Strongest`（24）は 8 スレッド環境で 1 手あたり最大 5 秒前後に収まる上限として選んでいる。
    pub fn level(self) -> u8 {
        match self {
            Self::Beginner => 1,
            Self::Intermediate => 3,
            Self::Advanced => 6,
            Self::Expert => 10,
            Self::Strongest => 24,
        }
    }
}

/// AI の思考強さの指定方法。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strength {
    /// プリセットによる指定。
    Preset(Preset),
    /// 探索レベル（`LEVEL_RANGE`）による直接指定。
    Level(u8),
    /// 1 手あたりの思考時間制限による指定。
    TimePerMove(Duration),
}

impl Default for Strength {
    fn default() -> Self {
        Self::Preset(Preset::Intermediate)
    }
}

/// 時間制限モード時に指定する上限探索レベル。
///
/// 制限時間に達するまで、このレベルを上限として反復深化（1 手ずつ読みを深くする探索）を続ける。
/// 終局まで届かない中盤でも、時間内であれば読み深さが伸び続ける。
pub const TIMED_LEVEL: u8 = 40;

/// 時間制限モードで選択可能な 1 手あたりの思考時間候補。
pub const MOVE_TIMES: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(30),
];
