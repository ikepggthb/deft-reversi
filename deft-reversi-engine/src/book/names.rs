//! 定石名。
//!
//! Edax にも Egaroucid にも無い。対局中に「この進行は 虎 です」と出せると
//! 人にとっての意味が段違いに増えるので、book が持つ情報の一部にしてある。
//!
//! 名前は局面ではなく **手順** に付く (同じ局面に複数の名前が集まる)。
//! 局面から引いたときは、その局面に到達する定石の名前をすべて返す。

use crate::board::board::Board;
use crate::board::position::position_str_to_num;
use crate::EngineError;
use std::collections::BTreeMap;

/// 定石 1 つ分。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opening {
    pub name: String,
    /// 初期局面からの手順 (例: `"F5D6C3D3C4"`)。
    pub record: String,
}

/// 定石名の表。正規形の盤面から名前の並びを引く。
#[derive(Clone, Debug, Default)]
pub struct NameTable {
    openings: Vec<Opening>,
    /// 正規形の盤面 -> その局面が終点になる定石の番号。
    exact: BTreeMap<Board, Vec<u32>>,
    /// 正規形の盤面 -> そこを通る定石の番号 (途中経過を含む)。
    passing: BTreeMap<Board, Vec<u32>>,
}

impl NameTable {
    pub fn is_empty(&self) -> bool {
        self.openings.is_empty()
    }

    pub fn len(&self) -> usize {
        self.openings.len()
    }

    pub fn openings(&self) -> &[Opening] {
        &self.openings
    }

    /// 定石を 1 つ登録する。手順が不正なら弾く。
    pub fn insert(&mut self, name: &str, record: &str) -> Result<(), EngineError> {
        let boards = replay(record)?;
        let id = self.openings.len() as u32;
        self.openings.push(Opening {
            name: name.to_string(),
            record: record.to_string(),
        });

        for (i, board) in boards.iter().enumerate() {
            let key = board.unique_board();
            let table = if i + 1 == boards.len() {
                &mut self.exact
            } else {
                &mut self.passing
            };
            let entry = table.entry(key).or_default();
            if !entry.contains(&id) {
                entry.push(id);
            }
        }
        Ok(())
    }

    /// この局面がちょうど終点になる定石の名前。
    pub fn names_at(&self, board: &Board) -> Vec<&str> {
        self.lookup(&self.exact, board)
    }

    /// この局面を通る定石の名前 (まだ途中のもの)。
    pub fn names_through(&self, board: &Board) -> Vec<&str> {
        self.lookup(&self.passing, board)
    }

    fn lookup<'a>(&'a self, table: &'a BTreeMap<Board, Vec<u32>>, board: &Board) -> Vec<&'a str> {
        table
            .get(&board.unique_board())
            .map(|ids| {
                ids.iter()
                    .map(|&id| self.openings[id as usize].name.as_str())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `名前 = 手順` 形式のテキストを読み込む。`//` と `#` はコメント。
    ///
    /// 既存の `opening.txt` (うえのん定石一覧) がこの形式。
    pub fn from_text(input: &str) -> Result<Self, EngineError> {
        let mut table = Self::default();
        for (i, line) in input.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
                continue;
            }
            let Some((name, record)) = line.split_once('=') else {
                continue;
            };
            let (name, record) = (name.trim(), record.trim());
            if name.is_empty() || record.is_empty() {
                continue;
            }
            table.insert(name, record).map_err(|err| {
                EngineError::InvalidData(format!("opening line {}: {err}", i + 1))
            })?;
        }
        Ok(table)
    }

    /// `名前 = 手順` 形式で書き出す。
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for opening in &self.openings {
            out.push_str(&opening.name);
            out.push_str(" = ");
            out.push_str(&opening.record);
            out.push('\n');
        }
        out
    }
}

/// 手順を再生して、通過した局面 (初期局面を除く) を返す。
fn replay(record: &str) -> Result<Vec<Board>, EngineError> {
    if record.len() % 2 != 0 {
        return Err(EngineError::InvalidRecord {
            reason: "record length must be even".to_string(),
            offset: record.len(),
        });
    }
    let mut board = Board::new();
    let mut boards = Vec::with_capacity(record.len() / 2);
    for (i, chunk) in record.as_bytes().chunks(2).enumerate() {
        let move_str = std::str::from_utf8(chunk).map_err(|_| EngineError::InvalidRecord {
            reason: "record is not UTF-8".to_string(),
            offset: i * 2,
        })?;
        if board.moves() == 0 {
            if board.opponent_moves() == 0 {
                return Err(EngineError::InvalidRecord {
                    reason: "game is already over".to_string(),
                    offset: i * 2,
                });
            }
            board = board.passed();
        }
        let mv = position_str_to_num(move_str)?;
        if board.moves() & (1u64 << mv) == 0 {
            return Err(EngineError::InvalidRecord {
                reason: format!("illegal move: {move_str}"),
                offset: i * 2,
            });
        }
        board = board.make_move(1u64 << mv);
        boards.push(board);
    }
    Ok(boards)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_found_at_the_end_of_the_line() {
        let mut table = NameTable::default();
        table.insert("虎", "F5D6C3D3C4").unwrap();

        let board = replay("F5D6C3D3C4").unwrap().pop().unwrap();
        assert_eq!(table.names_at(&board), vec!["虎"]);
        assert!(table.names_through(&board).is_empty());
    }

    #[test]
    fn names_are_found_along_the_line() {
        let mut table = NameTable::default();
        table.insert("虎", "F5D6C3D3C4").unwrap();

        let midway = replay("F5D6C3").unwrap().pop().unwrap();
        assert_eq!(table.names_through(&midway), vec!["虎"]);
        assert!(table.names_at(&midway).is_empty());
    }

    #[test]
    fn names_are_found_through_every_symmetry() {
        let mut table = NameTable::default();
        table.insert("虎", "F5D6C3D3C4").unwrap();
        let board = replay("F5D6C3D3C4").unwrap().pop().unwrap();

        for sym in board.all_symmetries() {
            assert_eq!(table.names_at(&sym), vec!["虎"]);
        }
    }

    #[test]
    fn several_openings_can_share_a_position() {
        let mut table = NameTable::default();
        table.insert("兎", "F5F6E6F4").unwrap();
        table.insert("兎 (別名)", "F5F6E6F4").unwrap();

        let board = replay("F5F6E6F4").unwrap().pop().unwrap();
        assert_eq!(table.names_at(&board).len(), 2);
    }

    #[test]
    fn text_round_trip_preserves_the_openings() {
        let text = "虎 = F5D6C3D3C4\n// コメント\n\n兎 = F5F6E6F4\n";
        let table = NameTable::from_text(text).unwrap();
        assert_eq!(table.len(), 2);

        let reloaded = NameTable::from_text(&table.to_text()).unwrap();
        assert_eq!(reloaded.openings(), table.openings());
    }

    #[test]
    fn illegal_records_are_rejected() {
        let mut table = NameTable::default();
        assert!(table.insert("だめ", "A1").is_err());
        assert!(table.insert("だめ", "F5D").is_err());
        assert!(NameTable::from_text("だめ = A1").is_err());
    }

    #[test]
    fn unknown_positions_have_no_names() {
        let table = NameTable::default();
        assert!(table.names_at(&Board::new()).is_empty());
        assert!(table.names_through(&Board::new()).is_empty());
    }
}
