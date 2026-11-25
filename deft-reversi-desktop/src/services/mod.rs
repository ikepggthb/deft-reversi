//! コントローラ（`app`）からの要求を受けて重い処理をバックグラウンドスレッドで実行するサービス群。
//!
//! 効果音再生（[`audio`]）、Book 参照・生成（[`book`], [`book_gen`]）、
//! 探索エンジン制御（[`engine`]）、棋譜・設定の永続化（[`storage`]）を提供する。
pub mod audio;
pub mod book;
pub mod book_gen;
pub mod engine;
pub mod storage;
