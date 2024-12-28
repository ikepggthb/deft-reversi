use crate::board::*;
use std::collections::BTreeMap;
use std::fs;

#[derive(Debug)]
pub struct UenonOpening {
    name: String,
    sequence: String,
}

// 定石データをパース
fn parse_uenon_openings(openings_string: &str) -> Vec<UenonOpening> {
    let mut openings = Vec::new();
    for line in openings_string.lines() {
        let trimmed_line = line.trim();

        // コメント行はスキップ
        if trimmed_line.starts_with("//") || trimmed_line.is_empty() {
            continue;
        }

        if let Some((name, sequence)) = trimmed_line.split_once('=') {
            openings.push(UenonOpening {
                name: name.trim().to_string(),
                sequence: sequence.trim().to_string(),
            });
        }
    }

    openings
}

pub struct OpeningBook {
    pub opening_names: Vec<String>,  // 定石名
    opening_boards: BTreeMap<Board, OpeningInfo>,     // 盤面, 定石データ
}

#[derive(Debug)]
pub struct OpeningInfo {
    pub name_index: Option<usize>,        // 定石名のインデックス
    pub reachable_indices: Vec<usize>,   // 到達可能な定石名のインデックス
}

#[derive(Debug)]
pub enum OpeningBookError {
    ParseError(String),
    InvalidOpeningData(String),
    IoError(std::io::Error),
}

impl std::fmt::Display for OpeningBookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpeningBookError::ParseError(msg) => write!(f, "Parsing error: {}", msg),
            OpeningBookError::InvalidOpeningData(msg) => write!(f, "Invalid opening data: {}", msg),
            OpeningBookError::IoError(err) => write!(f, "IO error: {}", err),
        }
    }
}

impl std::error::Error for OpeningBookError {}

impl OpeningBook {
    pub fn new() -> Self {
        Self {
            opening_names: vec![],
            opening_boards: BTreeMap::new()
        }
    }
    
    pub fn from_file(file_path: &str)  ->  Result<Self, OpeningBookError> {
        let s = fs::read_to_string(file_path).map_err(OpeningBookError::IoError)?;
        s.parse()
    }
    
    pub fn name_str_from_board(&self, board: &Board) -> Option<&str> {
        self.name_from_board(board).map(|i| self.opening_names[i].as_str())
    }

    pub fn name_from_board(&self, board: &Board)  -> Option<usize> {
        let u_board = board.get_unique_board();
         self.opening_boards.get(&u_board)
                            .and_then(|o_info| {
                                o_info.name_index
                            })
    }

    pub fn reachable_name_string(&self, board: &Board) -> Option<Vec<String>> {
        self.reachable_name(board).map(|vec_index| 
            vec_index.iter().map(|&i| self.opening_names[i].clone()).collect()
        )
    }

    pub fn reachable_name(&self, board: &Board) -> Option<Vec<usize>> {
        let u_board = board.get_unique_board();
        self.opening_boards.get(&u_board)
                           .map(|o_info|
            { o_info.reachable_indices.to_vec()})
    }

    pub fn opening_move(&self, board: &Board, name_index: usize) -> Result<Option<u64>, PutPieceErr> {
        let mut legal_moves = board.put_able();
        while legal_moves != 0 {
            let legal_move = (!legal_moves + 1) & legal_moves;
            legal_moves &= legal_moves - 1;
            let next_board = {
                let mut b = board.clone();
                b.put_piece(legal_move)?;
                b
            };

            if let Some(vec_name_index) = self.reachable_name(&next_board) {
                if vec_name_index.contains(&name_index) {
                    return Ok(Some(legal_move));
                }
            }
            if let Some(next_board_name) = self.name_from_board(&next_board) {
                if next_board_name == name_index {
                    return Ok(Some(legal_move));
                }
            }
        }

        Ok(None)
    }

    pub fn opening_move_from_string(&self, board: &Board, name: &String) -> Result<Option<u64>, PutPieceErr> {
        let mut legal_moves = board.put_able();
        while legal_moves != 0 {
            let legal_move = (!legal_moves + 1) & legal_moves;
            legal_moves &= legal_moves - 1;
            let next_board = {
                let mut b = board.clone();
                b.put_piece(legal_move)?;
                b
            };

            if let Some(vec_name) = self.reachable_name_string(&next_board) {
                if vec_name.contains(name) {
                    return Ok(Some(legal_move));
                }
            }
            if let Some(next_board_name) = self.name_str_from_board(&next_board) {
                if next_board_name == *name {
                    return Ok(Some(legal_move));
                }
            }
        }

        Ok(None)
    }


}

impl Default for OpeningBook {
    fn default() -> Self {
        Self::new()
    }
}

use std::str::FromStr;

impl FromStr for OpeningBook {
    type Err = OpeningBookError;

    fn from_str(openings_str: &str) -> Result<Self, Self::Err > {
        let mut book = Self::new();

        let openings = parse_uenon_openings(openings_str);

        for (i, opening) in openings.iter().enumerate() {
            book.opening_names.push(opening.name.clone());

            let mut board = Board::new();
            let mut positions = opening.sequence.as_bytes().chunks_exact(2).peekable();
            while let Some(position) = positions.next() {
                let position = std::str::from_utf8(position).map_err(|e| OpeningBookError::ParseError(format!("{}", e)))?;
                let position_bit = position_str_to_bit(position).map_err(|s| OpeningBookError::InvalidOpeningData(s.to_string()))?;
                board.put_piece(position_bit).map_err(|_| OpeningBookError::InvalidOpeningData(format!("record: {}, invalid position bit: {:<02b}", opening.sequence ,position_bit)))?;
                let u_board = board.get_unique_board();
                
                match book.opening_boards.get_mut(&u_board) {
                    Some(opening_info) => {
                        if positions.peek().is_none() {// 棋譜の最後 (boardは、棋譜の最後の盤面になる)
                            opening_info.name_index = Some(i);
                            break;
                        } else {
                            opening_info.reachable_indices.push(i);
                        }
                    }
                    None => {
                        book.opening_boards.insert(
                            u_board.clone(),
                            if positions.peek().is_none() {// 棋譜の最後 (boardは、棋譜の最後の盤面になる)
                                OpeningInfo { 
                                    name_index: Some(i),
                                    reachable_indices: vec![] 
                                }
                            } else {
                                OpeningInfo { 
                                    name_index: None,
                                    reachable_indices: vec![i] 
                                }
                            }

                        );
                    }
                }

                if board.put_able() == 0 {
                    board.swap();
                }

            }
        }

        Ok(book)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    pub fn read_opening_txt() {
        // ファイルのパスを指定
        let file_path = "opening.txt";

        let s = match fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to parse file: {}", e);
                return;
            }
        };

        // 定石データをパース
        let openings = parse_uenon_openings(&s);
        for opening in openings {
            println!("{:?}", opening);
        }
    }
    #[test]
    fn opening_test() {
        use crate::Game;

        const FILE_PATH : &str = "opening.txt";

        const RECORD    : &str = "c4e3f6e6f5c5f4g6f7"; //花形定石ローズビル
        const OPRNING   : &str = "花形定石ローズビル";
        const REACHABLE: [&str; 3] = ["FJT", "酉", "金魚"]; //花形定石ローズビルからの変形

        let opening_book = OpeningBook::from_file(FILE_PATH).unwrap();
        let game = Game::from_record(RECORD).unwrap();
        
        if let Some(s) = opening_book.name_str_from_board(&game.current.board) {
            println!("定石:\n    {s}");
            assert_eq!(s, OPRNING)
        }
        println!("ここからたどり着ける定石: ");
        if let Some(names) = opening_book.reachable_name_string(&game.current.board) {
            for name in names.iter() {
                println!("    {}", name);
            }
            assert!(
                REACHABLE.iter()
                         .all(
                            |&r| names.iter().any(|name| name.contains(r)))
            );
        }
    }

    #[test]
    fn read_and_print_opening_boards() -> Result<(), OpeningBookError> {

        let opening_book = OpeningBook::from_file("opening.txt")?;
        // 定石ファイルを読み込んで opening_boards を初期化

        // opening_boards の内容を出力
        for (i, (_, info)) in opening_book.opening_boards.iter().enumerate() {
            println!("Opening #{}:", i);
            // 定石名を出力
            if let Some(name_index) = info.name_index {
                println!("Name: {}", opening_book.opening_names[name_index]);
            } else {
                println!("Name: None");
            }

            println!("Reachable Indices: ");
            info.reachable_indices.iter().for_each(|&i| {println!("{}, ", opening_book.opening_names[i])});

            // 区切り線
            println!("--------------------------");
        }

        Ok(())
    }

    #[test]
    fn output_opening_names_with_index() -> Result<(), OpeningBookError>  {
    
        let opening_book = OpeningBook::from_file("opening.txt")?;
        for (index, name) in opening_book.opening_names.iter().enumerate() {
            print!("[");
            print!("{}, \"{}\"", index, name);
            println!("],");
        }
        
        Ok(())
    }
}