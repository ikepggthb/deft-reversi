//! Ctrl-C シグナルを受け取り、実行中の探索を停止するモジュール。
//!
//! 探索の実行中に Ctrl-C が押された場合は [`StopHandle`] を通じて探索を中断し、
//! 探索が動いていない場合は終了コード 130 でプロセスを終了する。

use deft_reversi_engine::StopHandle;
use std::sync::Mutex;

/// 現在 Ctrl-C で停止対象になっている探索のハンドル。
static CURRENT: Mutex<Option<StopHandle>> = Mutex::new(None);

/// Ctrl-C ハンドラをプロセス全体に登録する。
///
/// 登録に失敗した場合は警告を標準エラー出力へ出し、処理は続行する。
pub fn install() {
    let installed = ctrlc::set_handler(|| match &*CURRENT.lock().unwrap() {
        Some(stop) => stop.stop(),
        None => std::process::exit(130),
    });
    if let Err(e) = installed {
        eprintln!("warning: Ctrl-C will not stop searches: {e}");
    }
}

/// 返却するガード [`Armed`] が破棄されるまで、Ctrl-C で `stop` を呼び出せるようにする。
pub fn arm(stop: StopHandle) -> Armed {
    *CURRENT.lock().unwrap() = Some(stop);
    Armed
}

/// [`arm`] が返す RAII ガード。スコープを抜けて破棄されると停止対象を解除する。
pub struct Armed;

impl Drop for Armed {
    fn drop(&mut self) {
        *CURRENT.lock().unwrap() = None;
    }
}
