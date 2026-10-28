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

#[test]
fn mpc_confirmation_must_be_shallower_and_rejection_is_atomic() {
    let valid = MpcConfig::default();
    valid.validate().unwrap();
    let mut engine = Engine::new(
        PatternModel::zero(),
        EngineOptions {
            threads: 1,
            tt_mib: 1,
        },
    )
    .unwrap();
    engine.set_mpc_config(valid.clone()).unwrap();
    for (exact, depth) in [(false, 4), (false, 60), (true, 12), (true, 60)] {
        for confirmation in [depth as i32, 60] {
            let mut invalid = valid.clone();
            if exact {
                invalid.final_search.search_lv_by_empties[depth] = confirmation;
            } else {
                invalid.eval_search.search_lv_by_depth[depth] = confirmation;
            }
            assert!(invalid.validate().is_err());
            assert!(engine.set_mpc_config(invalid).is_err());
            assert_eq!(
                engine.mpc_config().unwrap().eval_search.search_lv_by_depth,
                valid.eval_search.search_lv_by_depth
            );
            assert_eq!(
                engine
                    .mpc_config()
                    .unwrap()
                    .final_search
                    .search_lv_by_empties,
                valid.final_search.search_lv_by_empties
            );
        }
    }
    // Unused table entries are retained for compatibility with legacy files.
    let mut unused = valid;
    unused.eval_search.search_lv_by_depth[3] = 60;
    unused.final_search.search_lv_by_empties[11] = 60;
    unused.validate().unwrap();
}
