//! Writes the main-branch ProbCut rules as `rules.json`.
use deft_reversi_engine::{Engine, EngineOptions, Model};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let model = std::env::args().nth(1).ok_or("usage: dumprules MODEL")?;
    let mut e = Engine::new(Model::from_file(model)?, EngineOptions::default().tt_mib(1))?;
    e.set_legacy_main_mpc()?;
    let mut bytes = Vec::new();
    e.write_analysis_settings(&mut bytes)?;
    let mut out = Vec::new();
    for r in bytes[1..].chunks(28) {
        let f = |i: usize| f64::from_le_bytes(r[4 + 8 * i..12 + 8 * i].try_into().unwrap());
        out.push(format!(
            "{{\"exact\":{},\"depth\":{},\"empties\":{},\"shallow_depth\":{},\"slope\":{:?},\"intercept\":{:?},\"deviation\":{:?}}}",
            r[0] != 0, r[1], r[2], r[3], f(0), f(1), f(2)
        ));
    }
    println!("[{}]", out.join(",\n"));
    Ok(())
}
