//! Loads a model with its ProbCut calibration: `MODEL` uses the main-branch
//! rules, `MODEL@RULES.json` the calibration written by
//! `deft-reversi-train mpc`. `MODEL@RULES.json:normal` uses those rules with
//! the normal deviation multiples instead of the measured ones.
use deft_reversi_engine::{Engine, EngineOptions, Model, ProbCutRule, ProbCutSigmas};
use std::error::Error;

#[derive(serde::Deserialize)]
struct Rule {
    exact: bool,
    depth: u8,
    empties: u8,
    shallow_depth: u8,
    slope: f64,
    intercept: f64,
    deviation: f64,
}

#[derive(serde::Deserialize)]
struct Sigmas {
    midgame: [f64; 6],
    exact: [f64; 6],
}

/// `rules.json`: an object with the multiples, or (older files) the rules
/// alone.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Calibration {
    Full { sigmas: Sigmas, rules: Vec<Rule> },
    Rules(Vec<Rule>),
}

pub fn engine(spec: &str, options: EngineOptions) -> Result<Engine, Box<dyn Error>> {
    let (model, rules) = match spec.split_once('@') {
        Some((m, r)) => (m, Some(r)),
        None => (spec, None),
    };
    let mut e = Engine::new(Model::from_file(model)?, options)?;
    let Some(path) = rules else {
        e.set_legacy_main_mpc()?;
        return Ok(e);
    };
    let (path, normal) = match path.strip_suffix(":normal") {
        Some(p) => (p, true),
        None => (path, false),
    };
    let (sigmas, rules) = match serde_json::from_slice(&std::fs::read(path)?)? {
        Calibration::Full { sigmas, rules } => (
            ProbCutSigmas {
                midgame: sigmas.midgame,
                exact: sigmas.exact,
            },
            rules,
        ),
        Calibration::Rules(rules) => (ProbCutSigmas::NORMAL, rules),
    };
    let rules = rules
        .into_iter()
        .map(|r| ProbCutRule {
            exact: r.exact,
            depth: r.depth,
            empties: r.empties,
            shallow_depth: r.shallow_depth,
            slope: r.slope,
            intercept: r.intercept,
            deviation: r.deviation,
        })
        .collect();
    let sigmas = if normal {
        ProbCutSigmas::NORMAL
    } else {
        sigmas
    };
    e.set_probcut_calibration(rules, sigmas)?;
    Ok(e)
}
