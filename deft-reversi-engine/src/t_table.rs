use crate::board::*;
use rand::Rng;

const TABLE_SIZE: usize = 1 << 22;
pub const N_TT_MOVES: usize = 2;

#[derive(Clone)]
pub struct TableData {
    pub board: Board,
    pub max: i8,
    pub min: i8,
    pub lv: u8,
    pub selectivity_lv: u8,
    pub moves: [u8; N_TT_MOVES],
    is_old: bool,
}

pub struct TranspositionTable {
    table: Vec<Option<TableData>>,
    rand_table: Box<[u32; 1 << 16]>,
}

impl Default for TranspositionTable {
    fn default() -> Self {
        let rand_table: Box<[u32; 1 << 16]> = Self::gen_rand_table();
        Self {
            table: vec![None; TABLE_SIZE],
            rand_table,
        }
    }
}

impl TranspositionTable {
    pub fn new() -> Self {
        Self::default()
    }
    fn gen_rand_table() -> Box<[u32; 1 << 16]> {
        // let mut rng = rand::thread_rng();
        let seed: [u8; 32] = [13; 32];
        let mut rng: rand::rngs::StdRng = rand::SeedableRng::from_seed(seed);
        // let mut rng = rand::SeedableRng::from_seed(10);
        let mut table = Box::new([0; 1 << 16]);

        for ti in table.iter_mut() {
            *ti = rng.gen_range(0..TABLE_SIZE as u32);
        }

        table
    }

    #[inline(always)]
    pub fn hash_board(&self, board: &Board) -> usize {
        let player_board_bit = board.player;
        let opponent_board_bit = board.opponent;

        (self.rand_table[(player_board_bit & 0xFFFF) as usize]
            ^ self.rand_table[((player_board_bit >> 16) & 0xFFFF) as usize]
            ^ self.rand_table[((player_board_bit >> 32) & 0xFFFF) as usize]
            ^ self.rand_table[((player_board_bit >> 48) & 0xFFFF) as usize]
            ^ self.rand_table[((opponent_board_bit >> 48) & 0xFFFF) as usize]
            ^ self.rand_table[((opponent_board_bit >> 32) & 0xFFFF) as usize]
            ^ self.rand_table[((opponent_board_bit >> 16) & 0xFFFF) as usize]
            ^ self.rand_table[(opponent_board_bit & 0xFFFF) as usize]) as usize
    }

    #[inline(always)]
    pub fn add(
        &mut self,
        board: &Board,
        min: i32,
        max: i32,
        lv: i32,
        selectivity_lv: i32,
        best_move: u8,
    ) {
        #[cfg(debug_assertions)]
        {
            const MAX: i32 = i8::MAX as i32;
            const MIN: i32 = i8::MIN as i32;
            assert!(
                MIN <= min && min <= max && max <= MAX,
                "in function t_table::add(), min: {min}, max: {max}, lv: {lv}, best move: {best_move}"
            );
        }

        let index = self.hash_board(board);

        // エントリの更新処理（共通）
        let update_entry = |entry: &mut TableData| {
            entry.max = max as i8;
            entry.min = min as i8;
            entry.lv = lv as u8;
            entry.selectivity_lv = selectivity_lv as u8;
            if entry.moves[0] != best_move && best_move != NO_COORD {
                entry.moves[1] = entry.moves[0];
                entry.moves[0] = best_move;
            }
            entry.is_old = false;
        };

        // 新規エントリ生成
        let new_entry = || TableData {
            board: board.clone(),
            max: max as i8,
            min: min as i8,
            lv: lv as u8,
            selectivity_lv: selectivity_lv as u8,
            moves: [best_move, NO_COORD],
            is_old: false,
        };

        // 1. まず現在のハッシュ位置を確認
        match self.table[index].as_mut() {
            Some(entry)
                if entry.board.player == board.player && entry.board.opponent == board.opponent =>
            {
                update_entry(entry);
                return;
            }
            Some(entry) if entry.is_old => {
                self.table[index] = Some(new_entry());
                return;
            }
            None => {
                self.table[index] = Some(new_entry());
                return;
            }
            _ => {} // 存在はするが、ボードが異なる場合
        }

        // 2. 次のインデックスを調べる
        let index = (index + 1) % TABLE_SIZE;
        match self.table[index].as_mut() {
            Some(entry)
                if entry.board.player == board.player && entry.board.opponent == board.opponent =>
            {
                update_entry(entry);
            }
            Some(entry) if entry.is_old => {
                self.table[index] = Some(new_entry());
            }
            Some(entry)
                if lv < entry.lv as i32
                    || (lv == entry.lv as i32 && selectivity_lv < entry.selectivity_lv as i32) =>
            {
                // エントリが古くなく、かつ置換表の lv, selectivity_lv より劣るなら何もしない
            }
            _ => {
                self.table[index] = Some(new_entry());
            }
        }
    }

    #[inline(always)]
    pub fn get(&self, board: &Board) -> Option<&TableData> {
        let index = self.hash_board(board);

        let mut td = self.table[index]
            .as_ref()
            .filter(|&x| x.board.player == board.player && x.board.opponent == board.opponent);

        if td.is_none() {
            let index = (index + 1) % TABLE_SIZE;
            td = self.table[index]
                .as_ref()
                .filter(|&x| x.board.player == board.player && x.board.opponent == board.opponent);
        }

        td
    }

    pub fn set_old(&mut self) {
        for t in self.table.iter_mut().flatten() {
            t.is_old = true;
        }
    }

    pub fn count_used_tt(&self) -> usize {
        let mut i = 0;
        for e in self.table.iter() {
            if e.is_some() {
                i += 1;
            }
        }
        i
    }
}
