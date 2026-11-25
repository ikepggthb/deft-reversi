//! 探索設定（レベル、評価探索、終盤ステージ、MPC 選択率）の解析・展開・表示を行うモジュール。
//!
//! CLI 引数や対話コマンドで指定された文字列を [`SearchPlan`] や [`Selectivity`] に変換し、
//! 現在の空きマス数に応じた実行条件やバージョン一覧を整形する。

use deft_reversi_engine::{Goal, SearchPlan, Selectivity};

/// 選択率（MPC＝Multi-ProbCut 枝刈りの信頼度パーセント）を表す文字列を [`Selectivity`] に変換する。
///
/// 末尾の `%` は省略できる。指定可能な値は `83`、`85`、`95`、`98`、`99`、`100`（枝刈りなしの完全探索）のみ。
pub fn selectivity(text: &str) -> Result<Selectivity, String> {
    let percent = text.trim_end_matches('%').parse::<u8>().ok();
    Selectivity::ALL
        .into_iter()
        .find(|s| Some(s.percent()) == percent)
        .ok_or_else(|| "MPC must be 83, 85, 95, 98, 99 or 100 (100 = no pruning)".into())
}

/// 終盤探索へ切り替える 1 段階分の設定（`PERCENT:EMPTIES` 形式）。
#[derive(Clone, Debug)]
pub struct EndgameStage {
    /// この段階で使う MPC 選択率（`100%` は完全読み）。
    pub selectivity: Selectivity,
    /// この段階を有効にする空きマス数の上限（0〜60。0 は無効）。
    pub empties: u8,
}

/// `PERCENT:EMPTIES` 形式（例: `95:24`）の文字列を [`EndgameStage`] に変換する。
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

/// [`Goal`] をエンジン内部で扱う [`SearchPlan`] に展開する。
pub fn plan(goal: Goal) -> SearchPlan {
    match goal {
        Goal::Level(n) => SearchPlan::level(n).expect("validated level"),
        Goal::Depth(n) => SearchPlan::midgame(n, Selectivity::Full),
        Goal::Endgame => SearchPlan::midgame(60, Selectivity::Full).endgame(Selectivity::Full, 60),
        Goal::Plan(plan) => plan,
    }
}

/// [`Goal`] の中盤探索設定と各終盤ステージの切り替え空きマス数を複数行のテキストに整形する。
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

/// 評価探索の簡略指定（`DEPTH[@PERCENT]` 形式）を解析した結果。
#[derive(Clone, Debug)]
pub struct EvalSpec {
    /// 中盤の評価探索深さ（0〜60）。
    pub depth: u8,
    /// 省略可能な MPC 選択率。省略時は既存の計画値を引き継ぐために使う。
    pub mpc: Option<Selectivity>,
}

/// `DEPTH[@PERCENT]` 形式（例: `12@83`）の文字列を [`EvalSpec`] に変換する。
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

/// [`Goal`] を 1 行の短い要約文字列（例: `eval 12@83; endgame 95:24,100:18`）に整形する。
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

/// 現在の空きマス数 `empties` に対して実際に適用される探索段階を文字列で返す。
pub fn active(goal: Goal, empties: u8) -> String {
    let p = plan(goal);
    // 複数の終盤ステージが重なる場合は、最も信頼度が高い（選択率の大きい）段階が優先されるため逆順に探す。
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

/// 探索目標と 1 手あたりの制限時間を 1 行の要約文字列にまとめる。
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

/// CLI・エンジン・Book ライブラリ・探索リビジョン・Git リビジョンの一覧文字列を返す。
pub fn versions() -> String {
    format!(
        "Versions: CLI {} | Engine {} | Book library {} | search revision {} | git {}",
        crate::VERSION,
        deft_reversi_engine::VERSION,
        deft_reversi_book::VERSION,
        deft_reversi_engine::SEARCH_SEMANTICS_REVISION,
        deft_reversi_engine::GIT_REVISION,
    )
}

/// 実行時バージョン、探索設定、エンジン構成（スレッド数・置換表サイズ・モデルパス）を標準エラー出力へ出す。
pub fn report(
    engine: &deft_reversi_engine::Engine,
    args: &crate::EngineArgs,
    goal: Goal,
    time: Option<std::time::Duration>,
) {
    let o = engine.options();
    eprintln!("{}", versions());
    eprintln!("Search: {}", summary(goal, time));
    eprintln!(
        "Engine: threads {}; main TT {} MiB (requested {}); model {}",
        o.threads,
        // エンジンの主置換表は指定値以下の最大の 2 のべき乗 MiB で確保される。
        1usize << o.tt_mib.ilog2(),
        o.tt_mib,
        args.model.display()
    );
}
