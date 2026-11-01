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
fn calibration_travels_with_the_model_and_bad_calibrations_are_rejected() {
    let plain = Model::from_pattern(PatternModel::zero());
    assert!(!plain.has_calibration());
    let mut sigmas = ProbCutSigmas::NORMAL;
    sigmas.midgame[2] = 1.3;
    let model = plain
        .clone()
        .with_calibration(legacy_main_rules(), sigmas)
        .unwrap();
    let mut bytes = Vec::new();
    model.write_to(&mut bytes).unwrap();
    let read = Model::from_bytes(&bytes).unwrap();
    assert!(read.has_calibration());

    // The engine takes the calibration from the model: the same settings as
    // installing the rules by hand.
    let options = EngineOptions::default().tt_mib(1);
    let from_file = Engine::new(read.clone(), options).unwrap();
    let mut by_hand = Engine::new(plain.clone(), options).unwrap();
    by_hand
        .set_probcut_calibration(legacy_main_rules(), sigmas)
        .unwrap();
    let settings = |e: &Engine| {
        let mut out = Vec::new();
        e.write_analysis_settings(&mut out).unwrap();
        out
    };
    assert_eq!(settings(&from_file), settings(&by_hand));

    // Without a calibration, selective searches are refused, full ones run.
    let mut bare = Engine::new(plain.clone(), options).unwrap();
    let selective = SearchRequest::new(Goal::Depth(4)).selectivity(Selectivity::Percent85);
    assert!(matches!(
        bare.search(Board::new(), &selective),
        Err(EngineError::InvalidCalibration(_))
    ));
    assert!(
        bare.search(Board::new(), &SearchRequest::new(Goal::Depth(4)))
            .is_ok()
    );
    bare.set_model(read).unwrap();
    assert!(bare.search(Board::new(), &selective).is_ok());

    // Invalid rules or multiples never make a model.
    let mut rules = legacy_main_rules();
    rules[0].shallow_depth = rules[0].depth;
    assert!(plain.clone().with_calibration(rules, sigmas).is_err());
    let mut rules = legacy_main_rules();
    rules.push(rules[0]);
    assert!(plain.clone().with_calibration(rules, sigmas).is_err());
    let mut bad = sigmas;
    bad.exact[0] = 3.0;
    assert!(plain.with_calibration(legacy_main_rules(), bad).is_err());
}
