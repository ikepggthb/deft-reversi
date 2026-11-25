//! ディスク上の永続化ファイル（棋譜・設定・パス解決・アトミック読み書き）を扱うモジュール。
//!
//! このモジュール配下の処理は GPUI に依存しない。
pub mod atomic_file;
pub mod paths;
pub mod record;
pub mod settings_file;
