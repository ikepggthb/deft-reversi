//! アプリケーションが使用する設定・棋譜・ロックファイルの配置先パスを解決するモジュール。
//!
//! 環境変数 `DEFT_REVERSI_HOME` が指定されている場合は、テストやポータブル運用向けに
//! すべてのファイルをそのディレクトリ配下に配置する。
use std::path::PathBuf;

/// アプリケーションが参照する主要ファイルのパス集合。
#[derive(Clone, Debug)]
pub struct Paths {
    /// 設定ファイル（`settings.json`）のパス。
    pub settings: PathBuf,
    /// 自動保存された棋譜（`.dreversi`）を格納するディレクトリ。
    pub games: PathBuf,
    /// 多重起動防止用ロックファイルのパス。
    pub lock: PathBuf,
}

impl Paths {
    /// 指定されたルートディレクトリ配下にすべてのパスを配置した [`Paths`] を返す。
    pub fn under(root: PathBuf) -> Self {
        Self {
            settings: root.join("settings.json"),
            games: root.join("games"),
            lock: root.join("deft-reversi.lock"),
        }
    }

    /// 環境変数または OS 標準のディレクトリから [`Paths`] を解決する。
    ///
    /// OS から設定・データディレクトリを取得できない場合は `None` を返し、保存を行わない。
    pub fn from_env() -> Option<Self> {
        if let Some(root) = std::env::var_os("DEFT_REVERSI_HOME").filter(|v| !v.is_empty()) {
            return Some(Self::under(root.into()));
        }
        let config = dirs::config_dir()?.join("deft-reversi");
        let data = dirs::data_local_dir()?.join("deft-reversi");
        Some(Self {
            settings: config.join("settings.json"),
            games: data.join("games"),
            lock: data.join("deft-reversi.lock"),
        })
    }

    /// 対局 ID に対応する `.dreversi` ファイルパスを返す。
    pub fn game(&self, id: &str) -> PathBuf {
        debug_assert!(crate::domain::GameInfo::valid_id(id));
        self.games.join(format!("{id}.dreversi"))
    }
}
