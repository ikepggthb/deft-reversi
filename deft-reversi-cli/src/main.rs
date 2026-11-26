//! `deft-reversi-cli` のエントリーポイント。
//!
//! 対話対戦（`play`）、Book 生成・検査（`book`）、問題集解析（`solve`）、棋譜検討（`review`）、
//! 自己対戦（`self-play`）、合法手展開数計測（`perft`）、GTP 対局（`gtp`）、
//! FlipSight 対局（`flipsight`）の各サブコマンドを振り分ける。

mod book;
mod book_view;
mod display;
mod flipsight;
mod game;
mod game_clock;
mod gtp;
mod perft;
mod play;
mod review;
mod self_play;
mod settings;
mod solve;
mod stop;

use clap::{Args, Parser, Subcommand};
use deft_reversi_engine::{Engine, EngineOptions, Goal, Model, SearchRequest};
use std::{error::Error, path::PathBuf, time::Duration};

/// CLI 全体で使う標準エラー型のエイリアス。
type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// パッケージのバージョン文字列。
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Git リビジョンを含めた詳細バージョン文字列。
static BUILD_VERSION: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("{VERSION} (git {})", deft_reversi_engine::GIT_REVISION));

/// ターミナルからリバーシの対局・解析・棋譜検討を行う CLI。
#[derive(Parser, Debug)]
#[command(author, version, long_version = BUILD_VERSION.as_str(), about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// サブコマンドの一覧。
#[derive(Subcommand, Debug)]
enum Command {
    /// エンジンと対局する。
    Play(play::PlayArgs),
    /// Book（`.dbook` / `.dbook-work`）の生成・再開・書き出し・検査を行う。
    #[command(disable_help_flag = true)]
    Book(book::BookArgs),
    /// OBF 形式の問題集ファイルを読み込み、全局面を探索して表形式で表示する。
    Solve(solve::SolveArgs),
    /// 棋譜を解析し、実際に打たれた手と最善手を比較する。
    Review(review::ReviewArgs),
    /// エンジン同士を自己対戦させ、棋譜を書き出す。
    SelfPlay(self_play::SelfPlayArgs),
    /// 初期局面からの末端局面数を数える（perft）。
    Perft(perft::PerftArgs),
    /// 自動対局向けの GTP（Go Text Protocol）インターフェースを起動する。
    Gtp(gtp::GtpArgs),
    /// FlipSight ボットコネクタ向けの JSON Lines プロトコルを起動する。
    Flipsight(flipsight::FlipsightArgs),
}

/// 配布アーカイブで実行ファイルと同じフォルダに置く評価モデルのファイル名。
const BUNDLED_MODEL: &str = "deft-eval-v4.dpr2";

/// `--model` も `DEFT_MODEL` もないときに読む、実行ファイルと同じフォルダの評価モデル。
fn bundled_model() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(BUNDLED_MODEL)))
        .unwrap_or_else(|| PathBuf::from(BUNDLED_MODEL))
}

/// 探索を行うコマンドで共通に使うモデルとエンジン設定。
#[derive(Args, Debug, Clone)]
struct EngineArgs {
    /// DPR2 形式の評価モデルファイル（省略時は実行ファイルと同じフォルダの deft-eval-v4.dpr2）。
    #[arg(short, long, env = "DEFT_MODEL", default_value_os_t = bundled_model())]
    model: PathBuf,
    /// 探索スレッド数（呼び出しスレッドを含む。省略時は利用可能な全コア）。
    #[arg(short, long)]
    threads: Option<usize>,
    /// 置換表（過去の探索結果を保持するハッシュ表）のサイズ（MiB）。
    #[arg(long, default_value_t = 256)]
    tt_mib: usize,
}

impl EngineArgs {
    /// 指定パスのモデルを読み込み、[`Engine`] を構築する。
    fn engine(&self) -> Result<Engine> {
        let model =
            Model::from_file(&self.model).map_err(|e| format!("{}: {e}", self.model.display()))?;
        self.with_model(model)
    }

    /// 読み込み済みの [`Model`] を使って [`Engine`] を構築する。
    fn with_model(&self, model: Model) -> Result<Engine> {
        let threads = self
            .threads
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
        let options = EngineOptions::default()
            .threads(threads)
            .tt_mib(self.tt_mib);
        Ok(Engine::new(model, options)?)
    }
}

/// 各探索でどこまで読むか（深さ・選択率・制限時間）を指定する共通引数。
#[derive(Args, Debug, Clone)]
struct GoalArgs {
    /// エンジンレベル（1〜60）。
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(1..=60))]
    level: Option<u8>,
    /// 固定探索深さ（0〜60）。
    #[arg(short, long, conflicts_with = "level", value_parser = clap::value_parser!(u8).range(0..=60))]
    depth: Option<u8>,
    /// 終局まで枝刈りなしで完全読みする。
    #[arg(long, conflicts_with_all = ["level", "depth"])]
    exact: bool,
    /// 評価探索の簡略指定: `DEPTH[@PERCENT]`（例: `12@83`）。
    #[arg(long, conflicts_with_all = ["depth", "exact", "eval_depth", "eval_mpc"], value_parser = settings::eval_spec)]
    eval: Option<settings::EvalSpec>,
    /// 評価探索の深さを上書きする（`--level` なしの場合は深さ 10・MPC 100%・終盤探索オフが基準）。
    #[arg(long, conflicts_with_all = ["depth", "exact"], value_parser = clap::value_parser!(u8).range(0..=60))]
    eval_depth: Option<u8>,
    /// 評価探索の MPC（選択率）: 83, 85, 95, 98, 99, 100（100 は枝刈りなし）。
    #[arg(long, conflicts_with_all = ["depth", "exact"], value_parser = settings::selectivity)]
    eval_mpc: Option<deft_reversi_engine::Selectivity>,
    /// 終盤ステージ `PERCENT:EMPTIES`（複数回またはカンマ区切りで指定可。0 で無効化）。
    #[arg(long, conflicts_with_all = ["depth", "exact"], value_delimiter = ',', value_parser = settings::endgame_stage)]
    endgame: Vec<settings::EndgameStage>,
    /// 1 探索あたりの時間制限（ミリ秒。0 で無制限）。
    #[arg(long)]
    time_ms: Option<u64>,
}

impl GoalArgs {
    /// 指定されたオプションから [`Goal`] を組み立てる。何も指定がない場合は `default` を返す。
    fn goal(&self, default: Goal) -> Goal {
        if self.eval.is_some()
            || self.eval_depth.is_some()
            || self.eval_mpc.is_some()
            || !self.endgame.is_empty()
        {
            let mut plan = self
                .level
                .map(|n| settings::plan(Goal::Level(n)))
                .unwrap_or_else(|| {
                    deft_reversi_engine::SearchPlan::midgame(
                        10,
                        deft_reversi_engine::Selectivity::Full,
                    )
                });
            plan = plan.with_midgame(
                self.eval
                    .as_ref()
                    .map(|e| e.depth)
                    .or(self.eval_depth)
                    .unwrap_or(plan.midgame_depth()),
                self.eval
                    .as_ref()
                    .and_then(|e| e.mpc)
                    .or(self.eval_mpc)
                    .unwrap_or(plan.midgame_selectivity()),
            );
            for stage in &self.endgame {
                plan = plan.endgame(stage.selectivity, stage.empties);
            }
            return Goal::Plan(plan);
        }
        match (self.level, self.depth, self.exact) {
            (Some(level), _, _) => Goal::Level(level),
            (_, Some(depth), _) => Goal::Depth(depth),
            (_, _, true) => Goal::Endgame,
            _ => default,
        }
    }

    /// 0 より大きい値が指定されている場合のみ制限時間 [`Duration`] を返す。
    fn time_limit(&self) -> Option<Duration> {
        self.time_ms.filter(|&ms| ms > 0).map(Duration::from_millis)
    }

    /// `goal` と制限時間を反映した [`SearchRequest`] を生成する。
    fn request(&self, goal: Goal) -> SearchRequest {
        let request = SearchRequest::new(goal);
        match self.time_ms {
            Some(ms) if ms > 0 => request.time_limit(Duration::from_millis(ms)),
            _ => request,
        }
    }
}

/// 対話モード内の `search` コマンドで [`GoalArgs`] をそのまま解析するためのラッパー。
#[derive(Parser)]
struct SearchCommand {
    #[command(flatten)]
    goal: GoalArgs,
}

// 引数がなければ `play` として扱う（配布版を Windows でダブルクリックしたときに対局を始めるため）。
// 旧来の `--solve FILE` / `-s FILE` 形式の呼び出しも `solve FILE` サブコマンドとして受け付けるための前処理。
fn normalize_args(mut args: Vec<std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    if args.len() == 1 {
        args.push("play".into());
        return args;
    }
    let known = [
        "play",
        "book",
        "solve",
        "review",
        "self-play",
        "perft",
        "gtp",
        "flipsight",
        "help",
    ];
    if args
        .get(1)
        .and_then(|s| s.to_str())
        .is_some_and(|s| known.contains(&s))
    {
        return args;
    }
    if let Some(i) = args.iter().position(|s| s == "--solve" || s == "-s") {
        if i + 1 < args.len() {
            let file = args.remove(i + 1);
            args.remove(i);
            args.insert(1, "solve".into());
            args.insert(2, file);
        }
    } else if let Some((i, file)) = args.iter().enumerate().find_map(|(i, s)| {
        s.to_str()?
            .strip_prefix("--solve=")
            .map(|f| (i, f.to_string()))
    }) {
        args.remove(i);
        args.insert(1, "solve".into());
        args.insert(2, file.into());
    }
    args
}

/// Ctrl-C ハンドラを登録し、引数を解析して各サブコマンドを実行する。
fn main() {
    stop::install();
    let result = match Cli::parse_from(normalize_args(std::env::args_os().collect())).command {
        Command::Play(args) => play::run(args),
        Command::Book(args) => book::run(args),
        Command::Solve(args) => solve::run(args),
        Command::Review(args) => review::run(args),
        Command::SelfPlay(args) => self_play::run(args),
        Command::Gtp(args) => gtp::run(args),
        Command::Flipsight(args) => flipsight::run(args),
        Command::Perft(args) => {
            perft::run(args);
            Ok(())
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_parse() {
        Cli::command_check();
        let cli = Cli::try_parse_from(["cli", "solve", "a.obf", "-m", "m.bin", "--exact"]).unwrap();
        assert!(matches!(cli.command, Command::Solve(_)));
        assert!(
            Cli::try_parse_from(["cli", "solve", "a.obf", "-m", "m", "-l", "3", "-d", "4"])
                .is_err()
        );
        assert!(Cli::try_parse_from(["cli", "play", "-m", "m", "-l", "61"]).is_err());
        let cli = Cli::try_parse_from(["cli", "perft", "9", "--count-pass"]).unwrap();
        assert!(matches!(cli.command, Command::Perft(_)));
    }

    #[test]
    fn custom_search_plans_parse_and_override_levels() {
        let cli = Cli::try_parse_from([
            "cli",
            "solve",
            "a.obf",
            "-m",
            "m",
            "--eval-depth",
            "12",
            "--eval-mpc",
            "83",
            "--endgame",
            "95:24,99:22,100:18",
        ])
        .unwrap();
        let Command::Solve(args) = cli.command else {
            unreachable!()
        };
        let p = settings::plan(args.goal.goal(Goal::Endgame));
        assert_eq!(p.midgame_depth(), 12);
        assert_eq!(p.midgame_selectivity().percent(), 83);
        assert_eq!(
            p.endgame_limit(deft_reversi_engine::Selectivity::Percent95),
            24
        );
        assert_eq!(p.endgame_limit(deft_reversi_engine::Selectivity::Full), 18);
        let cli = Cli::try_parse_from([
            "cli",
            "play",
            "-m",
            "m",
            "--level",
            "24",
            "--eval-depth",
            "7",
            "--endgame",
            "100:0",
        ])
        .unwrap();
        let Command::Play(args) = cli.command else {
            unreachable!()
        };
        let p = settings::plan(args.goal.goal(Goal::Level(10)));
        assert_eq!(p.midgame_depth(), 7);
        assert_eq!(p.endgame_limit(deft_reversi_engine::Selectivity::Full), 0);
        assert_eq!(
            p.endgame_limit(deft_reversi_engine::Selectivity::Percent95),
            settings::plan(Goal::Level(24))
                .endgame_limit(deft_reversi_engine::Selectivity::Percent95)
        );
        for flags in [
            vec!["--eval-mpc", "94"],
            vec!["--endgame", "99:61"],
            vec!["--endgame", "95"],
            vec!["--exact", "--eval-depth", "1"],
            vec!["--depth", "1", "--endgame", "100:4"],
        ] {
            let mut argv = vec!["cli", "play", "-m", "m"];
            argv.extend(flags);
            assert!(Cli::try_parse_from(argv).is_err());
        }
    }

    #[test]
    fn compact_and_legacy_solve_settings_match() {
        let argv = normalize_args(
            [
                "cli",
                "--model",
                "m",
                "--solve",
                "a.obf",
                "--eval",
                "12@83",
                "--endgame",
                "95:24,100:18",
                "--time-ms",
                "0",
            ]
            .into_iter()
            .map(Into::into)
            .collect(),
        );
        let Command::Solve(args) = Cli::try_parse_from(argv).unwrap().command else {
            panic!()
        };
        let p = settings::plan(args.goal.goal(Goal::Endgame));
        assert_eq!(p.midgame_depth(), 12);
        assert_eq!(p.midgame_selectivity().percent(), 83);
        assert_eq!(p.endgame_limit(deft_reversi_engine::Selectivity::Full), 18);
        assert_eq!(args.goal.time_limit(), None);
        for flags in [
            ["--eval", "12@94"],
            ["--eval", "61@100"],
            ["--eval", "12@83@95"],
        ] {
            assert!(
                Cli::try_parse_from(["cli", "play", "-m", "m"].into_iter().chain(flags)).is_err()
            );
        }
        let argv = normalize_args(
            ["cli", "--solve=a.obf", "-m", "m"]
                .into_iter()
                .map(Into::into)
                .collect(),
        );
        assert!(matches!(
            Cli::try_parse_from(argv).unwrap().command,
            Command::Solve(_)
        ));
    }

    impl Cli {
        fn command_check() {
            use clap::CommandFactory;
            Self::command().debug_assert();
        }
    }
}
