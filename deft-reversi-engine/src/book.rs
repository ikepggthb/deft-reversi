use std::collections::HashMap;

use crate::board::{self, *};

pub struct OpeningBook {
    opening_names: Vec<String>,  // ソートされた (定石名, インデックス)
    opening_boards: Vec<(Board, OpeningInfo)>,     // ソートされた (盤面, 定石データ)
}

#[derive(Debug)]
pub struct OpeningInfo {
    pub name_index: Option<usize>,        // 定石名のインデックス
    pub reachable_indices: Vec<usize>,   // 到達可能な定石名のインデックス
}

impl OpeningBook {

    fn name(&self, board: &Board) -> String {
        match self.opening_boards.binary_search_by(|(a, _)| a.cmp(board)) {
            Ok(i) => {
                self.opening_names[self.opening_boards[i].1.name_index.unwrap()].clone()
            },
            Err(_) => {
                panic!()
            }
        }
    }
    fn reachable_name(&self, board: &Board) -> Vec<String> {
        let opening_board: &(Board, OpeningInfo) = match self.opening_boards.binary_search_by(|(a, _)| a.cmp(board)) {
            Ok(i) => {
                &self.opening_boards[i]
            },
            Err(_) => {
                panic!()
            }
        };
        let mut names = Vec::new();
        for &i in opening_board.1.reachable_indices.iter() {
            names.push(self.opening_names[i].clone());
        }
        names
    }
}