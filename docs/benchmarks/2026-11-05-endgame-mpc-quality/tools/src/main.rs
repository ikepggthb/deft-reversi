//! Scores Deft's selective endgame moves by exact solves.
//!
//! usage: mpc-quality MODEL POSITIONS.obf SELECTIVITY CACHE.tsv OUT.tsv
//!
//! For each position: a `Goal::Endgame` search at SELECTIVITY (68, 77, 85,
//! 95, 98 or 99) on a cleared TT, then the exact value of the position and
//! of the position after the chosen move (cached in CACHE.tsv, keyed by the
//! position text). The loss is the root value minus the chosen move's value.
//! Writes `index empties move loss seconds nodes`.
use deft_reversi_engine::{
    Action, Engine, EngineOptions, Goal, Model, Position, SearchRequest, Selectivity,
};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
};

fn exact(engine: &mut Engine, pos: Position, cache: &mut HashMap<String, i32>, file: &mut fs::File) -> i32 {
    let key = pos.to_string();
    if let Some(&v) = cache.get(&key) {
        return v;
    }
    engine.clear_tt().unwrap();
    let v = engine
        .search(pos, &SearchRequest::new(Goal::Endgame))
        .unwrap()
        .score
        .unwrap();
    writeln!(file, "{key}\t{v}").unwrap();
    cache.insert(key, v);
    v
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let selectivity = match a[3].as_str() {
        "68" => Selectivity::Percent68,
        "77" => Selectivity::Percent77,
        "85" => Selectivity::Percent85,
        "95" => Selectivity::Percent95,
        "98" => Selectivity::Percent98,
        "99" => Selectivity::Percent99,
        s => return Err(format!("selectivity {s}").into()),
    };
    let model = Model::from_file(&a[1])?;
    let options = EngineOptions::default().threads(1).tt_mib(256);
    let mut selective = Engine::new(model.clone(), options.clone())?;
    let mut solver = Engine::new(model, options)?;
    let mut cache: HashMap<String, i32> = fs::read_to_string(&a[4])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.rsplit_once('\t').map(|(k, v)| (k.to_string(), v.parse().unwrap())))
        .collect();
    let mut cache_file = OpenOptions::new().create(true).append(true).open(&a[4])?;
    let mut out = fs::File::create(&a[5])?;
    writeln!(out, "index\tempties\tmove\tloss\tseconds\tnodes")?;
    for (i, line) in fs::read_to_string(&a[2])?.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let pos: Position = line.trim().parse()?;
        selective.clear_tt()?;
        let r = selective.search(pos, &SearchRequest::new(Goal::Endgame).selectivity(selectivity))?;
        let action = r.best_move.unwrap_or(Action::Pass);
        let root = exact(&mut solver, pos, &mut cache, &mut cache_file);
        let child = exact(&mut solver, pos.play(action)?, &mut cache, &mut cache_file);
        let loss = root + child;
        writeln!(
            out,
            "{i}\t{}\t{action}\t{loss}\t{:.6}\t{}",
            pos.empty_count(),
            r.elapsed.as_secs_f64(),
            r.stats.nodes
        )?;
    }
    Ok(())
}
