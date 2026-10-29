//! Allocation regression for the actual public play path, including symmetry lookup.
use deft_reversi_engine::{book::*, *};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
struct Allocator;
thread_local! {static TRACK:Cell<bool>=const{Cell::new(false)};}
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
fn track() {
    if TRACK.try_with(Cell::get).unwrap_or(false) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
}
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        track();
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        track();
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        track();
        unsafe { System.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
#[test]
fn book_hit_performs_no_heap_allocations() {
    let engine = Engine::new(
        PatternModel::zero(),
        EngineOptions {
            threads: 1,
            tt_mib: 1,
        },
    )
    .unwrap();
    let mut editor = BookEditor::for_engine(&engine).unwrap();
    let board = Board::new();
    let action = Action::Place(Square::new(board.get_moves().trailing_zeros() as u8).unwrap());
    editor.register_move(board, action).unwrap();
    editor
        .record(
            board,
            action,
            Assessment {
                score: 0,
                quality: AnalysisQuality {
                    depth: 12,
                    selectivity: Selectivity::Full,
                    solved: false,
                },
                bound: Bound::Exact,
                identity: editor.identity(),
                origin: ValueOrigin::Search,
            },
        )
        .unwrap();
    let book = Arc::new(editor.export().unwrap());
    let mut player = BookPlayer::new(
        engine,
        Some(book),
        UsePolicy {
            moves: MovePolicy::RandomWithinLoss { max_loss: 2 },
            ..UsePolicy::default()
        },
        1,
    )
    .unwrap();
    let request = SearchRequest::new(board, SearchMode::Depth(1));
    TRACK.with(|t| t.set(true));
    for _ in 0..100 {
        let result = player.choose_move(request.clone()).unwrap();
        assert!(matches!(result.source, PlaySource::Book(_)));
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), 0);
}
