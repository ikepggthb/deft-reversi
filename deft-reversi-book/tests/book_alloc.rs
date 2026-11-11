//! Allocation regression for the actual public play path, including symmetry lookup.
use deft_reversi_book::*;
use deft_reversi_engine::*;
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
    let mut engine = Engine::new(
        Model::from_pattern(PatternModel::zero()),
        EngineOptions::default().tt_mib(1),
    )
    .unwrap();
    let mut editor = BookEditor::for_engine(&engine).unwrap();
    let board = Board::new();
    let action = Action::Place(board.legal_moves().next().unwrap());
    editor.register_move(board, action).unwrap();
    editor
        .record_candidate(
            board,
            action,
            Assessment::import(ImportedValue {
                score: 0,
                quality: AnalysisQuality {
                    depth: 12,
                    selectivity: Selectivity::Full,
                    solved: false,
                },
                bound: Bound::Exact,
                identity: editor.identity(),
                origin: ValueOrigin::Search,
            })
            .unwrap(),
        )
        .unwrap();
    let book = Arc::new(editor.export().unwrap());
    let mut player = BookPlayer::new(
        &engine,
        Some(book),
        UsePolicy {
            legacy: LegacyPolicy::RecordedQuality,
            moves: MovePolicy::RandomWithinLoss { max_loss: 2 },
            ..UsePolicy::default()
        },
        1,
    )
    .unwrap();
    let root = board.canonical();
    let action = Action::Place(root.legal_moves().next().unwrap());
    let request = SearchRequest::new(Goal::Depth(2)).root_moves(if let Action::Place(s) = action {
        1u64 << s.index()
    } else {
        0
    });
    let result = engine.search(root, &request).unwrap();
    let mut typed = BookEditor::for_engine(&engine).unwrap();
    typed.register_move(root, action).unwrap();
    typed
        .record_candidate(
            root,
            action,
            Assessment::from_search_result(
                root,
                RequestedAnalysis::External,
                SearchKind::Midgame {
                    depth: 2,
                    selectivity: Selectivity::Full,
                },
                &result,
                typed.identity(),
            )
            .unwrap()
            .project(SearchProjection::BestMove)
            .unwrap(),
        )
        .unwrap();
    let mut typed = BookPlayer::new(
        &engine,
        Some(Arc::new(typed.export().unwrap())),
        UsePolicy {
            min_depth: 2,
            min_selectivity: Selectivity::Full,
            ..Default::default()
        },
        1,
    )
    .unwrap();
    TRACK.with(|t| t.set(true));
    for _ in 0..100 {
        assert!(player.choose(board).is_some());
        assert!(typed.choose(board).is_some());
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), 0);
}
