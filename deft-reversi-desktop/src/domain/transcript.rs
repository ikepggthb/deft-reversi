//! `f5d6c3d3c4` のような 1 行の着手列文字列と [`History`] を相互変換するモジュール。
//!
//! クリップボードへのコピーや貼り付けによる棋譜の受け渡しに使用する。
//! パスは着手位置の連続から一意に決まるため文字列には含めない。
use super::{Grouping, History};
use deft_reversi_engine::{Action, Position, Square};

/// 棋譜全体からパスを除いた着手列を小文字の座標文字列（例: `f5d6c3`）へ変換する。
pub fn to_text(actions: &[Action]) -> String {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Place(square) => Some(square.to_string().to_lowercase()),
            Action::Pass => None,
        })
        .collect()
}

/// 初期局面からの棋譜テキストを解析し、[`History`] を構築する。
///
/// 英数字以外の文字（空白やハイフン等）は無視されるため、区切り文字を含む文字列もそのまま貼り付けられる。
/// 不正な手が含まれていた場合は、最初に打てなかった手の 1 始まりの手数を `Err` で返す。
pub fn parse(text: &str) -> Result<History, usize> {
    let chars: Vec<char> = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let mut history = History::new(Position::initial());
    for (i, pair) in chars.chunks(2).enumerate() {
        let square = pair.iter().collect::<String>().parse::<Square>().ok();
        let played = square.is_some_and(|square| {
            history
                .play(Action::Place(square), Grouping::NewGroup)
                .is_ok()
        });
        if !played {
            return Err(i + 1);
        }
    }
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_go_out_and_come_back() {
        let history = parse("F5 d6-c3 d3c4").unwrap();
        assert_eq!(history.actions().len(), 5);
        assert_eq!(to_text(history.actions()), "f5d6c3d3c4");
        assert_eq!(parse("f5f5").unwrap_err(), 2);
        assert_eq!(parse("f5d").unwrap_err(), 2);
        assert_eq!(parse("z9").unwrap_err(), 1);
        assert!(parse("").unwrap().actions().is_empty());
    }
}
