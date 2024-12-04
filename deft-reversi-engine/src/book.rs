use std::collections::HashMap;

use crate::board::*;



pub struct OpeningBook {
    opening_names: Vec<String>,  // ソートされた (定石名, インデックス)
    states: Vec<(Board, OpeningState)>,     // ソートされた (盤面, 定石データ)
}

#[derive(Debug)]
pub struct OpeningState {
    pub name_index: Option<usize>,        // 定石名のインデックス
    pub reachable_indices: Vec<usize>,   // 到達可能な定石名のインデックス
}