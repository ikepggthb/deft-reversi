use deft_reversi_engine::{Goal, SearchPlan, Selectivity};

pub fn selectivity(text: &str) -> Result<Selectivity, String> {
    let percent = text.trim_end_matches('%').parse::<u8>().ok();
    Selectivity::ALL
        .into_iter()
        .find(|s| Some(s.percent()) == percent)
        .ok_or_else(|| "MPC must be 83, 85, 95, 98, 99 or 100 (100 = no pruning)".into())
}

#[derive(Clone, Debug)]
pub struct EndgameStage {
    pub selectivity: Selectivity,
    pub empties: u8,
}

pub fn endgame_stage(text: &str) -> Result<EndgameStage, String> {
    let (percent, empties) = text
        .split_once(':')
        .ok_or("use PERCENT:EMPTIES, e.g. 95:24")?;
    let empties = empties.parse::<u8>().map_err(|_| "empties must be 0-60")?;
    if empties > 60 {
        return Err("empties must be 0-60".into());
    }
    Ok(EndgameStage {
        selectivity: selectivity(percent)?,
        empties,
    })
}

pub fn plan(goal: Goal) -> SearchPlan {
    match goal {
        Goal::Level(n) => SearchPlan::level(n).expect("validated level"),
        Goal::Depth(n) => SearchPlan::midgame(n, Selectivity::Full),
        Goal::Endgame => SearchPlan::midgame(60, Selectivity::Full).endgame(Selectivity::Full, 60),
        Goal::Plan(plan) => plan,
    }
}

pub fn describe(goal: Goal) -> String {
    let p = plan(goal);
    let mut out = format!(
        "Eval search: depth {}, MPC {}%\nEndgame (at or below empties):\n",
        p.midgame_depth(),
        p.midgame_selectivity().percent()
    );
    for s in Selectivity::ALL {
        let limit = p.endgame_limit(s);
        out.push_str(&format!(
            "  {}%: {}\n",
            s.percent(),
            if limit == 0 {
                "off".into()
            } else {
                limit.to_string()
            }
        ));
    }
    out.push_str("When stages overlap, the highest percentage applies. 100% = exact.\n");
    out
}

#[derive(Clone, Debug)]
pub struct EvalSpec {
    pub depth: u8,
    pub mpc: Option<Selectivity>,
}

pub fn eval_spec(text: &str) -> Result<EvalSpec, String> {
    let (depth, percent) = text
        .split_once('@')
        .map_or((text, None), |(d, p)| (d, Some(p)));
    let depth = depth
        .parse::<u8>()
        .map_err(|_| "use DEPTH[@PERCENT], e.g. 12@83")?;
    if depth > 60 {
        return Err("eval depth must be 0-60".into());
    }
    Ok(EvalSpec {
        depth,
        mpc: percent.map(selectivity).transpose()?,
    })
}

pub fn compact(goal: Goal) -> String {
    match goal {
        Goal::Level(n) => format!("level {n} ({})", compact(Goal::Plan(plan(goal)))),
        Goal::Depth(n) => format!("eval {n}@100; endgame off"),
        Goal::Endgame => "exact".into(),
        Goal::Plan(p) => {
            let stages: Vec<_> = Selectivity::ALL
                .into_iter()
                .filter_map(|s| {
                    let e = p.endgame_limit(s);
                    (e > 0).then(|| format!("{}:{e}", s.percent()))
                })
                .collect();
            format!(
                "eval {}@{}; endgame {}",
                p.midgame_depth(),
                p.midgame_selectivity().percent(),
                if stages.is_empty() {
                    "off".into()
                } else {
                    stages.join(",")
                }
            )
        }
    }
}

pub fn active(goal: Goal, empties: u8) -> String {
    let p = plan(goal);
    match Selectivity::ALL
        .into_iter()
        .rev()
        .find(|&s| p.endgame_limit(s) > 0 && empties <= p.endgame_limit(s))
    {
        Some(Selectivity::Full) => "exact".into(),
        Some(s) => format!("endgame {}%", s.percent()),
        None => format!(
            "eval depth {}{}, MPC {}%",
            p.midgame_depth().min(empties),
            if p.midgame_depth() >= empties {
                " (reaches end)"
            } else {
                ""
            },
            p.midgame_selectivity().percent()
        ),
    }
}

pub fn summary(goal: Goal, time: Option<std::time::Duration>) -> String {
    format!(
        "{}; time {}",
        compact(goal),
        time.map_or_else(
            || "unlimited".into(),
            |t| format!("{} ms/search", t.as_millis())
        )
    )
}

pub fn report(
    engine: &deft_reversi_engine::Engine,
    args: &crate::EngineArgs,
    goal: Goal,
    time: Option<std::time::Duration>,
) {
    let o = engine.options();
    eprintln!("Search: {}", summary(goal, time));
    eprintln!(
        "Engine: threads {}; main TT {} MiB (requested {}); model {}",
        o.threads,
        1usize << o.tt_mib.ilog2(),
        o.tt_mib,
        args.model.display()
    );
}
