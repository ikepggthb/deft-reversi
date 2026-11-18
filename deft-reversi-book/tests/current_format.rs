//! The files were produced by the implementation before the evaluation refactor.
use deft_reversi_book::{Book, BookEditor, ReadLimits};
use std::path::Path;

#[test]
fn existing_v8_work_and_v4_play_keep_the_same_bytes_when_saved() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let out = std::env::temp_dir().join(format!("deft-format-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let work = fixtures.join("search-v8.work");
    let play = fixtures.join("search-v4.book");
    BookEditor::load(&work, ReadLimits::default())
        .unwrap()
        .save(out.join("copy.work"))
        .unwrap();
    Book::load(&play, ReadLimits::default())
        .unwrap()
        .save(out.join("copy.book"))
        .unwrap();
    assert_eq!(
        std::fs::read(work).unwrap(),
        std::fs::read(out.join("copy.work")).unwrap()
    );
    assert_eq!(
        std::fs::read(play).unwrap(),
        std::fs::read(out.join("copy.book")).unwrap()
    );
    std::fs::remove_dir_all(out).unwrap();
}
