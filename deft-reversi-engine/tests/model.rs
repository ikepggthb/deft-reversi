use deft_reversi_engine::*;
use std::io::Cursor;
#[test]
fn model_roundtrip_and_input_validation() {
    let original = PatternModel::zero();
    let mut bytes = Vec::new();
    original.write_to(&mut bytes).unwrap();
    let decoded = PatternModel::read_from(Cursor::new(&bytes)).unwrap();
    assert_eq!(
        decoded.evaluate(&Board::new()),
        original.evaluate(&Board::new())
    );
    assert!(PatternModel::read_from(Cursor::new(&bytes[..bytes.len() - 1])).is_err());
    bytes.push(0);
    assert!(PatternModel::read_from(Cursor::new(&bytes)).is_err());
    assert!(PatternModel::read_from(Cursor::new(b"unknown")).is_err());
    assert!(PatternModel::new(Vec::new()).is_err());
    assert!(PatternPhase::new(Vec::new(), [0; 128], 0).is_err());
}
