//! Loads a model with its ProbCut rules: `MODEL` uses the main-branch
//! rules, `MODEL@RULES.json` the rules written by `deft-reversi-train mpc`.
use deft_reversi_engine::{Engine, EngineOptions, Model, ProbCutRule};
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

pub fn engine(spec: &str, options: EngineOptions) -> Result<Engine, Box<dyn Error>> {
    let (model, rules) = match spec.split_once('@') {
        Some((m, r)) => (m, Some(r)),
        None => (spec, None),
    };
    let mut e = Engine::new(Model::from_file(model)?, options)?;
    match rules {
        None => e.set_legacy_main_mpc()?,
        Some(path) => {
            let rules: Vec<Rule> = serde_json::from_slice(&std::fs::read(path)?)?;
            e.set_probcut_rules(
                rules
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
                    .collect(),
            )?
        }
    }
    Ok(e)
}
