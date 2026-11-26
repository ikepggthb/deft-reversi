//! 使用した評価モデル・エンジンの情報（[`AnalysisProfile`]）と、Book 生成の実行履歴（[`GenerationRun`]）を管理するモジュール。
//!
//! どちらの履歴も作業ファイル（`.dbook-work`）と対局用ファイル（`.dbook`）の両方に保存される。
//! 局面や手ごとの実際の探索条件は `values.rs` の [`CompletedSearch`] が個別に持っており、この一覧には集約しない。
//! 旧形式ファイルからの変換元の関係は `legacy` モジュール内だけで扱い、ここには残さない。
use super::*;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalysisProfile {
    pub identity: AnalysisIdentity,
    pub engine_version: Option<String>,
    pub search_revision: Option<u32>,
    pub model_label: Option<String>,
    pub build_label: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Stopped(GenerationStop),
    Failed,
}
#[derive(Clone, Debug)]
pub struct InterruptedAnalysis {
    /// 中断された仕事の探索元。ここに残す値は診断用で、採用する候補評価ではない。
    pub root: Board,
    pub side: BookSide,
    pub stage: u8,
    pub requested: RequestedAnalysis,
    pub planned: SearchKind,
    pub moves: u64,
    pub completed: Option<SearchKind>,
    pub score: Option<i16>,
    pub reason: GenerationStop,
    pub nodes: u64,
}
#[derive(Clone, Debug)]
pub struct GenerationRun {
    pub engine_version: String,
    pub search_revision: u32,
    pub build_label: Option<String>,
    pub identity: AnalysisIdentity,
    pub config: GenerationConfig,
    pub roots: Vec<GenerationRoot>,
    pub started_unix_seconds: u64,
    pub threads: u16,
    pub main_tt_mib: u64,
    pub limits: GenerationLimits,
    pub status: RunStatus,
    pub nodes: u64,
    pub elapsed_micros: u64,
    pub interrupted: Vec<InterruptedAnalysis>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct Catalog {
    pub profiles: std::collections::BTreeMap<AnalysisIdentity, AnalysisProfile>,
    pub runs: Vec<GenerationRun>,
}
impl Catalog {
    pub fn engine(&mut self, identity: AnalysisIdentity) {
        let p = self.profiles.entry(identity).or_insert(AnalysisProfile {
            identity,
            engine_version: None,
            search_revision: None,
            model_label: None,
            build_label: None,
        });
        p.engine_version = Some(format!(
            "deft_reversi_engine/{}",
            deft_reversi_engine::VERSION
        ));
        p.search_revision = Some(deft_reversi_engine::SEARCH_SEMANTICS_REVISION);
    }
    pub fn bytes(&self) -> u64 {
        (self.profiles.len() * 256 + self.runs.capacity() * std::mem::size_of::<GenerationRun>())
            as u64
            + self
                .profiles
                .values()
                .map(|p| {
                    p.engine_version.as_ref().map_or(0, String::capacity)
                        + p.model_label.as_ref().map_or(0, String::capacity)
                        + p.build_label.as_ref().map_or(0, String::capacity)
                })
                .sum::<usize>() as u64
            + self
                .runs
                .iter()
                .map(|r| {
                    (r.engine_version.capacity()
                        + r.build_label.as_ref().map_or(0, String::capacity)
                        + r.roots.capacity() * std::mem::size_of::<GenerationRoot>()
                        + r.interrupted.capacity() * std::mem::size_of::<InterruptedAnalysis>()
                        + r.config.analysis.bytes()) as u64
                })
                .sum::<u64>()
    }
}
