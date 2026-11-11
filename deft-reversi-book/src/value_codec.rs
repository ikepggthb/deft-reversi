//! 評価の部品のバイナリ表現と、旧形式のAssessmentの読書き・ハッシュ計算。
//!
//! 現行work/playの共有一覧はvalues.rsがここにある部品の関数を使って書く。
//! write/read/fingerprintは主に旧形式の検査用。特に旧入力IDの再計算に使うため、
//! 既存のタグ番号・順序を変えると旧ファイルの検査結果も変わる。
use super::*;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
pub(super) fn byte(w: &mut dyn Write, n: u8) -> Result<()> {
    w.write_all(&[n])?;
    Ok(())
}
pub(super) fn read_byte(r: &mut impl Read) -> Result<u8> {
    let mut b = [0];
    r.read_exact(&mut b)?;
    Ok(b[0])
}
pub(super) fn n64(w: &mut dyn Write, n: u64) -> Result<()> {
    w.write_all(&n.to_le_bytes())?;
    Ok(())
}
pub(super) fn r64(r: &mut impl Read) -> Result<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}
pub(super) fn rootw(w: &mut dyn Write, p: [u64; 2]) -> Result<()> {
    n64(w, p[0])?;
    n64(w, p[1])
}
pub(super) fn rootr(r: &mut impl Read) -> Result<[u64; 2]> {
    Ok([r64(r)?, r64(r)?])
}
pub(super) fn scorew(w: &mut dyn Write, v: i16) -> Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}
pub(super) fn scorer(r: &mut impl Read) -> Result<i16> {
    let mut b = [0; 2];
    r.read_exact(&mut b)?;
    Ok(i16::from_le_bytes(b))
}
pub(super) fn idr(r: &mut impl Read) -> Result<[u8; 32]> {
    let mut b = [0; 32];
    r.read_exact(&mut b)?;
    Ok(b)
}
pub(super) fn boolr(r: &mut impl Read) -> Result<bool> {
    match read_byte(r)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid("boolean")),
    }
}
pub(super) fn originw(v: ValueOrigin) -> u8 {
    match v {
        ValueOrigin::Search => 0,
        ValueOrigin::Manual => 1,
        ValueOrigin::Imported => 2,
        ValueOrigin::Rules => 3,
        ValueOrigin::Derived => 4,
    }
}
pub(super) fn originr(v: u8) -> Result<ValueOrigin> {
    match v {
        0 => Ok(ValueOrigin::Search),
        1 => Ok(ValueOrigin::Manual),
        2 => Ok(ValueOrigin::Imported),
        3 => Ok(ValueOrigin::Rules),
        4 => Ok(ValueOrigin::Derived),
        _ => Err(invalid("origin")),
    }
}
pub(super) fn kindw(w: &mut dyn Write, v: SearchKind) -> Result<()> {
    match v {
        SearchKind::Midgame { depth, selectivity } => {
            byte(w, 0)?;
            byte(w, depth)?;
            byte(w, selectivity as u8)
        }
        SearchKind::Endgame { selectivity } => {
            byte(w, 1)?;
            byte(w, selectivity as u8)
        }
    }
}
pub(super) fn kindr(r: &mut impl Read) -> Result<SearchKind> {
    match read_byte(r)? {
        0 => Ok(SearchKind::Midgame {
            depth: read_byte(r)?,
            selectivity: selectivity(read_byte(r)?)?,
        }),
        1 => Ok(SearchKind::Endgame {
            selectivity: selectivity(read_byte(r)?)?,
        }),
        _ => Err(invalid("search kind")),
    }
}
pub(super) fn requestedw(w: &mut dyn Write, v: RequestedAnalysis) -> Result<()> {
    match v {
        RequestedAnalysis::External => byte(w, 0),
        RequestedAnalysis::ChildLevel(n) => {
            byte(w, 1)?;
            byte(w, n)
        }
        RequestedAnalysis::ParentDepth { depth, selectivity } => {
            byte(w, 2)?;
            byte(w, depth)?;
            byte(w, selectivity as u8)
        }
        RequestedAnalysis::Endgame { selectivity } => {
            byte(w, 3)?;
            byte(w, selectivity as u8)
        }
    }
}
pub(super) fn requestedr(r: &mut impl Read) -> Result<RequestedAnalysis> {
    match read_byte(r)? {
        0 => Ok(RequestedAnalysis::External),
        1 => Ok(RequestedAnalysis::ChildLevel(read_byte(r)?)),
        2 => Ok(RequestedAnalysis::ParentDepth {
            depth: read_byte(r)?,
            selectivity: selectivity(read_byte(r)?)?,
        }),
        3 => Ok(RequestedAnalysis::Endgame {
            selectivity: selectivity(read_byte(r)?)?,
        }),
        _ => Err(invalid("requested analysis")),
    }
}
pub(super) fn summaryw(w: &mut dyn Write, q: QualitySummary) -> Result<()> {
    for v in [
        q.families,
        q.horizon,
        q.mid_depth,
        q.mid_sel,
        q.end_sel,
        q.legacy_depth,
        q.legacy_sel,
    ] {
        byte(w, v)?;
    }
    Ok(())
}
pub(super) fn summaryr(r: &mut impl Read) -> Result<QualitySummary> {
    let q = QualitySummary {
        families: read_byte(r)?,
        horizon: read_byte(r)?,
        mid_depth: read_byte(r)?,
        mid_sel: read_byte(r)?,
        end_sel: read_byte(r)?,
        legacy_depth: read_byte(r)?,
        legacy_sel: read_byte(r)?,
    };
    q.validate()?;
    Ok(q)
}
pub(super) fn write(w: &mut dyn Write, value: &Assessment) -> Result<()> {
    match value.claim() {
        ValueClaim::Estimate { score, bound } => {
            byte(w, 0)?;
            scorew(w, score)?;
            byte(
                w,
                match bound {
                    EstimateBound::Point => 0,
                    EstimateBound::Lower => 1,
                    EstimateBound::Upper => 2,
                },
            )?;
        }
        ValueClaim::Certified(v) => {
            byte(w, 1)?;
            scorew(w, v.lower().unwrap_or(127))?;
            scorew(w, v.upper().unwrap_or(127))?;
        }
    }
    w.write_all(&value.identity())?;
    match value.analysis() {
        ValueAnalysis::LegacyUnknown {
            depth,
            selectivity,
            recorded_origin,
        } => {
            byte(w, 0)?;
            byte(w, depth)?;
            byte(w, selectivity as u8)?;
            byte(w, originw(recorded_origin))?;
        }
        ValueAnalysis::Search { trace, projection } => {
            byte(w, 1)?;
            byte(
                w,
                match projection {
                    SearchProjection::Maximum => 0,
                    SearchProjection::BestMove => 1,
                    SearchProjection::OtherMoveUpper => 2,
                },
            )?;
            requestedw(w, trace.requested)?;
            kindw(w, trace.planned)?;
            kindw(w, trace.completed)?;
            rootw(w, trace.root)?;
            n64(w, trace.moves)?;
            byte(w, trace.best)?;
            scorew(w, trace.result_score)?;
            w.write_all(&trace.run.unwrap_or(u32::MAX).to_le_bytes())?;
            n64(w, trace.nodes)?;
            n64(w, trace.elapsed_micros)?;
        }
        ValueAnalysis::Derived { basis, summary } => {
            byte(w, 2)?;
            match basis {
                Derivation::Current { root, subject } => {
                    byte(w, 3)?;
                    rootw(w, root)?;
                    byte(w, subject.code())?;
                }
                Derivation::Maximum {
                    root,
                    policy,
                    inputs,
                } => {
                    byte(w, 0)?;
                    rootw(w, root)?;
                    byte(w, policy)?;
                    w.write_all(&inputs)?;
                }
                Derivation::Intersection {
                    root,
                    action,
                    inputs,
                } => {
                    byte(w, 2)?;
                    rootw(w, root)?;
                    byte(w, action)?;
                    w.write_all(&inputs[0])?;
                    w.write_all(&inputs[1])?;
                }
                Derivation::Child {
                    child,
                    input,
                    placement,
                } => {
                    byte(w, 1)?;
                    rootw(w, child)?;
                    w.write_all(&input)?;
                    byte(w, u8::from(placement))?;
                }
            }
            summaryw(w, summary)?;
        }
        ValueAnalysis::Rules { root } => {
            byte(w, 3)?;
            rootw(w, root)?;
        }
        ValueAnalysis::Manual { depth, selectivity } => {
            byte(w, 4)?;
            byte(w, depth)?;
            byte(w, selectivity as u8)?;
        }
    }
    Ok(())
}
pub(super) fn read(r: &mut impl Read) -> Result<Assessment> {
    let claim = match read_byte(r)? {
        0 => {
            let score = scorer(r)?;
            let bound = match read_byte(r)? {
                0 => EstimateBound::Point,
                1 => EstimateBound::Lower,
                2 => EstimateBound::Upper,
                _ => return Err(invalid("estimate bound")),
            };
            ValueClaim::Estimate { score, bound }
        }
        1 => {
            let l = scorer(r)?;
            let u = scorer(r)?;
            ValueClaim::Certified(ScoreInterval::new(
                (l != 127).then_some(l),
                (u != 127).then_some(u),
            )?)
        }
        _ => return Err(invalid("claim")),
    };
    let identity = idr(r)?;
    let analysis = match read_byte(r)? {
        0 => ValueAnalysis::LegacyUnknown {
            depth: read_byte(r)?,
            selectivity: selectivity(read_byte(r)?)?,
            recorded_origin: originr(read_byte(r)?)?,
        },
        1 => {
            let projection = match read_byte(r)? {
                0 => SearchProjection::Maximum,
                1 => SearchProjection::BestMove,
                2 => SearchProjection::OtherMoveUpper,
                _ => return Err(invalid("projection")),
            };
            let requested = requestedr(r)?;
            let planned = kindr(r)?;
            let completed = kindr(r)?;
            let root = rootr(r)?;
            let moves = r64(r)?;
            let best = read_byte(r)?;
            let result_score = scorer(r)?;
            let mut id = [0; 4];
            r.read_exact(&mut id)?;
            let id = u32::from_le_bytes(id);
            ValueAnalysis::Search {
                trace: CompletedSearch {
                    requested,
                    planned,
                    completed,
                    root,
                    moves,
                    best,
                    result_score,
                    run: (id != u32::MAX).then_some(id),
                    nodes: r64(r)?,
                    elapsed_micros: r64(r)?,
                },
                projection,
            }
        }
        2 => {
            let basis = match read_byte(r)? {
                3 => Derivation::Current {
                    root: rootr(r)?,
                    subject: CalculationSubject::from_code(read_byte(r)?)?,
                },
                0 => Derivation::Maximum {
                    root: rootr(r)?,
                    policy: read_byte(r)?,
                    inputs: idr(r)?,
                },
                1 => Derivation::Child {
                    child: rootr(r)?,
                    input: idr(r)?,
                    placement: boolr(r)?,
                },
                2 => Derivation::Intersection {
                    root: rootr(r)?,
                    action: read_byte(r)?,
                    inputs: [idr(r)?, idr(r)?],
                },
                _ => return Err(invalid("derivation")),
            };
            ValueAnalysis::Derived {
                basis,
                summary: summaryr(r)?,
            }
        }
        3 => ValueAnalysis::Rules { root: rootr(r)? },
        4 => ValueAnalysis::Manual {
            depth: read_byte(r)?,
            selectivity: selectivity(read_byte(r)?)?,
        },
        _ => return Err(invalid("analysis")),
    };
    Assessment::new(claim, analysis, identity)
}
pub(super) fn fingerprint(value: &Assessment) -> Result<[u8; 32]> {
    struct Sink(Sha256);
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.update(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut sink = Sink(Sha256::new());
    write(&mut sink, value)?;
    Ok(sink.0.finalize().into())
}
