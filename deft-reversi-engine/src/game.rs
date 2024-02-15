use crate::board::Board;


#[derive(Clone)]
pub struct RecordElement {
    put_place: u64,
}

#[derive(Clone)]
pub struct Game {
    pub record: Vec<RecordElement>,
    pub current_index: usize
}

impl Default for Game {
    fn default() -> Self {    
        let mut r = Self { record: Vec::new(), current_index: 0 };
        r.record.push(RecordElement {board: Board::new(), put_place: 0});
        r
    }
}

impl Game {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn current_board(&self) -> &Board {
        &self.record[self.current_index].board
    }

    pub fn undo(&mut self) {
        if self.current_index > 0 {
            self.current_index -= 1;
        }
    }
    
    pub fn redo(&mut self) {
        if self.current_index < (self.record.len()-1) {
            self.current_index += 1;
        }
    }

    pub fn add(&mut self, board: Board, put_place: u64){
        self.current_index += 1;
        self.record[self.current_index] = RecordElement{board, put_place};
    }

    pub fn reset(&mut self) {
        self.record.clear();
        self.record.push(RecordElement {board: Board::new(), put_place: 0});
        self.current_index = 0;
    }
}