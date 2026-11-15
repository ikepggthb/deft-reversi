use deft_reversi_engine::{Model, PHASE_COUNT, PatternModel, PatternPhase, PenPhase};
use std::io;

/// A DPR2 model from the phases `phase` returns. A phase it has no weights
/// for (the dataset has no positions with that many discs) copies the
/// nearest phase that has them, the earlier one on a tie.
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

/// A DPR2 version 3 model: linear phases with their networks. Missing
/// phases copy the nearest trained one, as in `assemble`.
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
