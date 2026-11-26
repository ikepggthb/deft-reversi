//! 学習済みのフェーズごとの重みをまとめ、エンジンが読み込める DPR2 形式のモデルバイト列に変換するモジュール。
//!
//! リバーシでは盤面の進行度（石数）によって石の価値が変わるため、石数ごとにフェーズを分けて重みを学習する。
//! データセットに局面が存在せず未学習となったフェーズについては、最も近い学習済みフェーズの重み（および補助ニューラルネット）を
//! コピーして補完する。

use deft_reversi_engine::{Model, PHASE_COUNT, PatternModel, PatternPhase, PenPhase};
use std::io;

/// `phase` が返す各フェーズの重みから DPR2 形式のモデルを組み立てる。
///
/// 重みが存在しないフェーズ（データセットに該当石数の局面がない場合など）は、
/// 最も近い学習済みフェーズの重みをコピーして補完する（同距離なら手前のフェーズを優先）。
pub fn assemble(phase: &mut dyn FnMut(usize) -> Option<PatternPhase>) -> io::Result<Vec<u8>> {
    let trained: Vec<Option<PatternPhase>> = (0..PHASE_COUNT).map(phase).collect();
    if trained.iter().all(Option::is_none) {
        return Err(io::Error::other("no trained phase"));
    }
    let nearest = |p: usize| {
        (0..PHASE_COUNT)
            .flat_map(|d| [p.checked_sub(d), Some(p + d)])
            .flatten()
            .find_map(|q| trained.get(q).cloned().flatten())
            .unwrap()
    };
    let phases = (0..PHASE_COUNT).map(nearest).collect();
    let mut bytes = Vec::new();
    PatternModel::new(phases)
        .map_err(io::Error::other)?
        .write_to(&mut bytes)?;
    Ok(bytes)
}

/// 線形フェーズ重みと補助ニューラルネットから DPR2 モデルを組み立てる（version 4、石数などの全体入力を使わない場合は version 3）。
///
/// 未学習フェーズは [`assemble`] と同様に最も近い学習済みフェーズをコピーして補完する。
pub fn assemble_pen(
    linear: &[Option<PatternPhase>],
    networks: &[Option<PenPhase>],
) -> io::Result<Vec<u8>> {
    let trained: Vec<usize> = (0..PHASE_COUNT).filter(|&p| linear[p].is_some()).collect();
    if trained.is_empty() {
        return Err(io::Error::other("no trained phase"));
    }
    let nearest = |p: usize| *trained.iter().min_by_key(|&&q| (q.abs_diff(p), q)).unwrap();
    let phases = (0..PHASE_COUNT)
        .map(|p| linear[nearest(p)].clone().unwrap())
        .collect();
    let pen = (0..PHASE_COUNT)
        .map(|p| networks[nearest(p)].clone().unwrap())
        .collect();
    let model = PatternModel::new(phases)
        .and_then(|m| m.with_pen(pen))
        .map_err(io::Error::other)?;
    let mut bytes = Vec::new();
    Model::from_pattern(model).write_to(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deft_reversi_engine::{Board, PATTERN_WEIGHT_COUNT};

    fn bias(b: i16) -> PatternPhase {
        PatternPhase::from_flat(vec![0; PATTERN_WEIGHT_COUNT], [0; 128], b * 128).unwrap()
    }

    #[test]
    fn missing_phases_copy_the_nearest_trained_one() {
        // Phases 10 and 20 only; the initial board is phase 0.
        let bytes = assemble(&mut |p| match p {
            10 => Some(bias(1)),
            20 => Some(bias(2)),
            _ => None,
        })
        .unwrap();
        let model = PatternModel::read_from(&bytes[..]).unwrap();
        let mut board = Board::new();
        let mut scores = vec![];
        while !board.is_terminal() {
            scores.push((60 - board.empty_count(), model.evaluate(&board)));
            let moves = board.legal_moves();
            board = match moves.into_iter().next() {
                Some(square) => board.play(square.into()).unwrap(),
                None => board.play(deft_reversi_engine::Action::Pass).unwrap(),
            };
        }
        for (phase, score) in scores {
            assert_eq!(score, if phase <= 15 { 1 } else { 2 }, "phase {phase}");
        }
        assert!(assemble(&mut |_| None).is_err());
    }
}
