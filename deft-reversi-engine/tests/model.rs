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
    sigmas.midgame[0] = 1.3;
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
    let selective = SearchRequest::new(Goal::Depth(4)).selectivity(Selectivity::Percent83);
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

fn pen_phase(hidden_bias: i32, output_bias: i32) -> Result<PenPhase, &'static str> {
    PenPhase::new(
        vec![[1; PEN_DIM]; PATTERN_WEIGHT_COUNT],
        &[[1; PEN_INPUTS]; PEN_HIDDEN],
        [hidden_bias; PEN_HIDDEN],
        [3; PEN_HIDDEN],
        output_bias,
    )
}

#[test]
fn pen_biases_beyond_the_limit_are_rejected() {
    let limit = 1 << 24;
    assert!(pen_phase(limit, -limit).is_ok());
    assert!(pen_phase(limit + 1, 0).is_err());
    assert!(pen_phase(0, -limit - 1).is_err());
    assert!(pen_phase(i32::MIN, 0).is_err());
    assert!(pen_phase(0, i32::MAX).is_err());
}

#[test]
fn evaluation_model_output_includes_the_networks() {
    let linear = PatternModel::zero();
    let pen = |bias| {
        linear
            .clone()
            .with_pen(vec![pen_phase(0, bias).unwrap(); PHASE_COUNT])
            .unwrap()
    };
    let identity = |pattern: PatternModel| {
        let model = Model::from_pattern(pattern)
            .with_calibration(legacy_main_rules(), ProbCutSigmas::NORMAL)
            .unwrap();
        let engine = Engine::new(model, EngineOptions::default().tt_mib(1)).unwrap();
        let mut out = Vec::new();
        engine.write_evaluation_model(&mut out).unwrap();
        out
    };
    let (plain, with_pen, other_pen) =
        (identity(linear.clone()), identity(pen(0)), identity(pen(7)));
    // Without networks the output is the linear weights, as before.
    let mut v1 = Vec::new();
    linear.write_to(&mut v1).unwrap();
    assert_eq!(plain, v1);
    // Networks are part of the evaluation, so they change the output.
    assert_ne!(plain, with_pen);
    assert_ne!(with_pen, other_pen);
    // The output is a model file without a calibration.
    let read = Model::from_bytes(&with_pen).unwrap();
    assert!(!read.has_calibration());
    let board = Board::new();
    assert_eq!(read.pattern().evaluate(&board), pen(0).evaluate(&board));
}
