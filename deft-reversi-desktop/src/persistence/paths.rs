//! Where files go (§8.2 of the plan). `DEFT_REVERSI_HOME` puts everything
//! under one folder, for tests and portable use.
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Paths {
    pub settings: PathBuf,
    pub games: PathBuf,
    pub lock: PathBuf,
}

impl Paths {
    pub fn under(root: PathBuf) -> Self {
        Self {
            settings: root.join("settings.json"),
            games: root.join("games"),
            lock: root.join("deft-reversi.lock"),
        }
    }

    /// None when the OS gives no folders, in which case nothing is saved.
    pub fn from_env() -> Option<Self> {
        if let Some(root) = std::env::var_os("DEFT_REVERSI_HOME") {
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

    pub fn game(&self, id: &str) -> PathBuf {
        self.games.join(format!("{id}.dreversi"))
    }
}
