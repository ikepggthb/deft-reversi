//! 定跡名リスト（`assets/openings.txt`）を参照し、対局の進行に一致する最長の定跡名を判定するモジュール。
//!
//! リバーシの盤面は回転や裏返し（鏡映）をしても本質的に同じ形になる。
//! そのため定跡リストでは、4 通りある初手のうちすべてを `F5` 始まりに統一（正規化）して登録している。
//! 実際の対局が別の初手（`C4` / `E6` / `D3`）から始まった場合は、盤面を回転・反転して `F5` 基準に揃えてから照合する。
//! リストの各行は `定跡名 = F5D6C3…` 形式で記述され、`//` で始まる行はコメントとして扱う。
use deft_reversi_engine::{Action, Square};
use std::sync::OnceLock;

/// 埋め込まれた定跡定義テキスト。
const LIST: &str = include_str!("../../assets/openings.txt");

/// 1 件の定跡定義。
struct Opening {
    /// 定跡名。
    name: &'static str,
    /// `F5` 基準で正規化された手順。
    moves: Vec<Square>,
}

/// パース済みの定跡リストを遅延初期化して返す。
fn openings() -> &'static [Opening] {
    static OPENINGS: OnceLock<Vec<Opening>> = OnceLock::new();
    OPENINGS.get_or_init(|| LIST.lines().filter_map(parse_line).collect())
}

/// 定跡ファイル 1 行をパースし、有効な [`Opening`] であれば返す。
fn parse_line(line: &'static str) -> Option<Opening> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("//") {
        return None;
    }
    let (name, moves) = line.rsplit_once('=')?;
    let moves = moves.trim();
    if moves.len() % 2 != 0 {
        return None;
    }
    let moves = (0..moves.len())
        .step_by(2)
        .map(|i| moves.get(i..i + 2)?.parse::<Square>().ok())
        .collect::<Option<Vec<_>>>()?;
    Some(Opening {
        name: name.trim(),
        moves,
    })
}

/// 初手 `first` を `F5` に移す対称変換関数を返す。
///
/// 4 通りの変換はいずれも「2 回適用すると元に戻る（対合）」性質を持つため、
/// 逆変換を別途用意する必要がない。
fn to_f5(first: Square) -> Option<fn(Square) -> Square> {
    match (first.x(), first.y()) {
        (5, 4) => Some(|s| s),
        // C4: 180 度回転。
        (2, 3) => Some(|s| Square::from_xy(7 - s.x(), 7 - s.y()).expect("on the board")),
        // E6: 主対角線での鏡映。
        (4, 5) => Some(|s| Square::from_xy(s.y(), s.x()).expect("on the board")),
        // D3: 副対角線での鏡映。
        (3, 2) => Some(|s| Square::from_xy(7 - s.y(), 7 - s.x()).expect("on the board")),
        _ => None,
    }
}

/// 対局の着手列に前方一致する最長の定跡名を返す。
///
/// 通常の初期局面から始まり、かつパスを挟まずに定跡手順へ一致している場合のみ判定対象となる。
pub fn name(actions: &[Action]) -> Option<&'static str> {
    let Action::Place(first) = *actions.first()? else {
        return None;
    };
    let turn = to_f5(first)?;
    let moves: Vec<Square> = actions
        .iter()
        .map_while(|a| match a {
            Action::Place(s) => Some(turn(*s)),
            Action::Pass => None,
        })
        .collect();
    openings()
        .iter()
        .filter(|o| moves.starts_with(&o.moves))
        .max_by_key(|o| o.moves.len())
        .map(|o| o.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actions(moves: &str) -> Vec<Action> {
        (0..moves.len())
            .step_by(2)
            .map(|i| Action::Place(moves[i..i + 2].parse().unwrap()))
            .collect()
    }

    #[test]
    fn names_match_the_longest_line_in_every_orientation() {
        assert!(openings().len() > 250);
        assert_eq!(name(&actions("F5D6")), Some("縦取り"));
        assert_eq!(name(&actions("F5D6C3D3C4")), Some("虎定石"));
        // Moves past the listed line keep its name.
        assert_eq!(name(&actions("F5D6C3D3C4A1")), Some("虎定石"));
        // The same opening turned three ways.
        assert_eq!(name(&actions("C4E3F6E6F5")), Some("虎定石"));
        assert_eq!(name(&actions("E6F4C3C4D3")), Some("虎定石"));
        assert_eq!(name(&actions("D3C5F6F5E6")), Some("虎定石"));
        assert_eq!(name(&[]), None);
    }
}
