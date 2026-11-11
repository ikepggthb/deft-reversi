//! Versioned, bounded streaming codec. No Rust layout or unbounded serde vectors.
//!
//! 現行の書込はsave_editor（work v8）とsave_book（play v4）。読込側は旧版も扱う。
//! 現行の共有評価一覧はvalues.rs、評価の個々の部品と旧形式はvalue_codec.rsを使う。
//! 手数や盤面の検査に加え、旧形式の入力関係はvalidate_catalogで検査してから捨てる。
//! 形式を追う順序と保存項目はdocs/book-v2/source-review.mdにまとめている。
use super::*;
use super::{
    editor::{Edge, Frontier, Node, Pending},
    generate::{Context, GenerationState},
    snapshot::{Record, SnapshotMetadata, StoredCandidate, ValuePool},
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const BOOK: &[u8; 8] = b"DFTBK001";
const WORK: &[u8; 8] = b"DFTBW001";
// 8バイト識別子 + u32形式版 + u32ルール版 + u64本文長 + SHA-256(本文)32バイト。
// 数値はリトルエンディアン。Rustの構造体のメモリ配置はそのまま書かない。
const HEADER: u64 = 56;
static TEMP: AtomicU64 = AtomicU64::new(0);
struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
struct HashWriter<W> {
    inner: W,
    hash: Sha256,
    len: u64,
}
impl<W: Write> Write for HashWriter<W> {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(b)?;
        self.hash.update(&b[..n]);
        self.len = self
            .len
            .checked_add(n as u64)
            .ok_or_else(|| io::Error::other("book length overflow"))?;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
// 同じディレクトリの一時ファイルへ本文を書き、長さとハッシュをヘッダへ埋める。
// ファイルを同期してからrenameで置換し、Unixでは親ディレクトリも同期する。
// 書込途中のエラーでは既存の保存先を置き換えず、TempFileが一時ファイルを片付ける。
fn atomic_save(
    path: &Path,
    magic: &[u8; 8],
    version: u32,
    body: impl FnOnce(&mut dyn Write) -> Result<()>,
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| invalid("save path"))?
        .to_string_lossy();
    let (mut file, temp) = loop {
        let p = parent.join(format!(
            ".{name}.{}.{}.tmp",
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&p)
        {
            Ok(f) => break (f, TempFile(p)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    file.write_all(&[0; HEADER as usize])?;
    let mut out = HashWriter {
        inner: BufWriter::new(&mut file),
        hash: Sha256::new(),
        len: 0,
    };
    body(&mut out)?;
    out.flush()?;
    let len = out.len;
    let sum: [u8; 32] = out.hash.clone().finalize().into();
    drop(out);
    file.seek(SeekFrom::Start(0))?;
    file.write_all(magic)?;
    u32w(&mut file, version)?;
    u32w(&mut file, 1)?;
    u64w(&mut file, len)?;
    file.write_all(&sum)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp.0, path)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}
// 本文の構造を読む前に、種類・版・全体長・ハッシュを検査する。
// 正しいハッシュでも意味の正しさは保証しない。後段で件数・参照先・評価の意味を検査する。
fn open(path: &Path, magic: &[u8; 8], limits: ReadLimits) -> Result<(BufReader<File>, u32)> {
    let mut file = BufReader::new(File::open(path)?);
    let len = file.get_ref().metadata()?.len();
    if len < HEADER || len > limits.max_bytes {
        return Err(BookError::Limit("file size"));
    }
    let mut got = [0; 8];
    file.read_exact(&mut got)?;
    if &got != magic {
        return Err(invalid("file kind"));
    }
    let version = u32r(&mut file)?;
    if !(version == 1
        || (magic == BOOK && (2..=4).contains(&version))
        || (magic == WORK && (2..=8).contains(&version)))
        || u32r(&mut file)? != 1
    {
        return Err(invalid("unsupported format or rules"));
    }
    let payload = u64r(&mut file)?;
    if payload.checked_add(HEADER) != Some(len) {
        return Err(invalid("payload length"));
    }
    let mut expected = [0; 32];
    file.read_exact(&mut expected)?;
    let mut hash = Sha256::new();
    let mut bytes = [0; 16384];
    loop {
        let n = file.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    if <[u8; 32]>::from(hash.finalize()) != expected {
        return Err(invalid("checksum"));
    }
    file.seek(SeekFrom::Start(HEADER))?;
    Ok((file, version))
}
fn done(input: &mut impl Read) -> Result<()> {
    let mut b = [0];
    if input.read(&mut b)? != 0 {
        return Err(invalid("trailing data"));
    }
    Ok(())
}
fn u8w(w: &mut dyn Write, v: u8) -> Result<()> {
    w.write_all(&[v])?;
    Ok(())
}
fn u32w(w: &mut dyn Write, v: u32) -> Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}
fn u64w(w: &mut dyn Write, v: u64) -> Result<()> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}
fn u8r(r: &mut impl Read) -> Result<u8> {
    let mut b = [0];
    r.read_exact(&mut b)?;
    Ok(b[0])
}
fn boolr(r: &mut impl Read) -> Result<bool> {
    match u8r(r)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid("boolean")),
    }
}
fn u32r(r: &mut impl Read) -> Result<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn u64r(r: &mut impl Read) -> Result<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}
fn idr(r: &mut impl Read) -> Result<AnalysisIdentity> {
    let mut b = [0; 32];
    r.read_exact(&mut b)?;
    Ok(b)
}
fn keyw(w: &mut dyn Write, k: Key) -> Result<()> {
    u64w(w, k.0)?;
    u64w(w, k.1)
}
fn keyr(r: &mut impl Read) -> Result<Key> {
    let k = Key(u64r(r)?, u64r(r)?);
    let b = Board::from_bits(k.0, k.1).ok_or_else(|| invalid("overlapping stones"))?;
    if b.empty_count() > 60 {
        return Err(invalid("position has fewer than four stones"));
    }
    if Key::new(b).0 != k {
        return Err(invalid("noncanonical key"));
    }
    Ok(k)
}
fn stopw(w: &mut dyn Write, v: GenerationStop) -> Result<()> {
    u8w(
        w,
        match v {
            GenerationStop::Exhausted => 0,
            GenerationStop::Requested => 1,
            GenerationStop::TimeLimit => 2,
            GenerationStop::NodeLimit => 3,
            GenerationStop::PositionLimit => 4,
            GenerationStop::MemoryLimit => 5,
        },
    )
}
fn stopr(r: &mut impl Read) -> Result<GenerationStop> {
    match u8r(r)? {
        0 => Ok(GenerationStop::Exhausted),
        1 => Ok(GenerationStop::Requested),
        2 => Ok(GenerationStop::TimeLimit),
        3 => Ok(GenerationStop::NodeLimit),
        4 => Ok(GenerationStop::PositionLimit),
        5 => Ok(GenerationStop::MemoryLimit),
        _ => Err(invalid("stop reason")),
    }
}
fn catalogw(w: &mut dyn Write, c: &catalog::Catalog) -> Result<()> {
    u64w(w, c.profiles.len() as u64)?;
    for p in c.profiles.values() {
        w.write_all(&p.identity)?;
        stringw(w, p.engine_version.as_deref().unwrap_or(""))?;
        u32w(w, p.search_revision.unwrap_or(u32::MAX))?;
        stringw(w, p.model_label.as_deref().unwrap_or(""))?;
        stringw(w, p.build_label.as_deref().unwrap_or(""))?;
    }
    u64w(w, c.runs.len() as u64)?;
    for run in &c.runs {
        stringw(w, &run.engine_version)?;
        u32w(w, run.search_revision)?;
        stringw(w, run.build_label.as_deref().unwrap_or(""))?;
        w.write_all(&run.identity)?;
        configw(w, &run.config)?;
        u64w(w, run.roots.len() as u64)?;
        for r in &run.roots {
            contextw(
                w,
                Context {
                    key: Key::new(r.board).0,
                    side: r.side,
                },
            )?;
        }
        u64w(w, run.started_unix_seconds)?;
        u32w(w, u32::from(run.threads))?;
        u64w(w, run.main_tt_mib)?;
        u64w(
            w,
            run.limits.time.map_or(u64::MAX, |v| {
                v.as_micros().min(u128::from(u64::MAX - 1)) as u64
            }),
        )?;
        u64w(w, run.limits.nodes.unwrap_or(u64::MAX))?;
        u64w(w, run.limits.new_positions.unwrap_or(u64::MAX))?;
        match run.status {
            RunStatus::Running => u8w(w, 0)?,
            RunStatus::Stopped(reason) => {
                u8w(w, 1)?;
                stopw(w, reason)?;
            }
            RunStatus::Failed => u8w(w, 2)?,
        }
        u64w(w, run.nodes)?;
        u64w(w, run.elapsed_micros)?;
        u64w(w, run.interrupted.len() as u64)?;
        for j in &run.interrupted {
            contextw(
                w,
                Context {
                    key: Key::new(j.root).0,
                    side: j.side,
                },
            )?;
            u8w(w, j.stage)?;
            value_codec::requestedw(w, j.requested)?;
            value_codec::kindw(w, j.planned)?;
            u64w(w, j.moves)?;
            u8w(w, u8::from(j.completed.is_some()))?;
            if let Some(k) = j.completed {
                value_codec::kindw(w, k)?;
            }
            w.write_all(&j.score.unwrap_or(127).to_le_bytes())?;
            stopw(w, j.reason)?;
            u64w(w, j.nodes)?;
        }
    }
    u64w(w, 0)?;
    Ok(())
}
fn catalogr(r: &mut impl Read, limits: ReadLimits, modern: bool) -> Result<catalog::Catalog> {
    let mut c = catalog::Catalog::default();
    for _ in 0..limited_count(r, (limits.max_bytes / 128).min(65536) as usize)? {
        let identity = idr(r)?;
        let version = stringr(r)?;
        let revision = u32r(r)?;
        let label = stringr(r)?;
        let build = stringr(r)?;
        let p = AnalysisProfile {
            identity,
            engine_version: (!version.is_empty()).then_some(version),
            search_revision: (revision != u32::MAX).then_some(revision),
            model_label: (!label.is_empty()).then_some(label),
            build_label: (!build.is_empty()).then_some(build),
        };
        if c.profiles.insert(identity, p).is_some() {
            return Err(invalid("duplicate analysis profile"));
        }
    }
    for _ in 0..limited_count(
        r,
        (limits.max_bytes / 512).min(u64::from(u32::MAX)) as usize,
    )? {
        let engine_version = stringr(r)?;
        let search_revision = u32r(r)?;
        let build = stringr(r)?;
        let identity = idr(r)?;
        let config = configr(r, if modern { 8 } else { 7 })?;
        let mut roots = Vec::new();
        for _ in 0..limited_count(r, limits.max_positions.saturating_mul(2) as usize)? {
            let x = contextr(r)?;
            roots.push(GenerationRoot {
                board: x.key.board(),
                side: x.side,
            });
        }
        if roots.is_empty() {
            return Err(invalid("run has no roots"));
        }
        let started_unix_seconds = u64r(r)?;
        let threads = u32r(r)?;
        let main_tt_mib = u64r(r)?;
        if !(1..=256).contains(&threads) || main_tt_mib == 0 {
            return Err(invalid("run resources"));
        }
        let time = u64r(r)?;
        let nodes = u64r(r)?;
        let new_positions = u64r(r)?;
        let limits = GenerationLimits {
            time: (time != u64::MAX).then(|| Duration::from_micros(time)),
            nodes: (nodes != u64::MAX).then_some(nodes),
            new_positions: (new_positions != u64::MAX).then_some(new_positions),
        };
        limits.validate()?;
        let status = match u8r(r)? {
            0 => RunStatus::Running,
            1 => RunStatus::Stopped(stopr(r)?),
            2 => RunStatus::Failed,
            _ => return Err(invalid("run status")),
        };
        let nodes = u64r(r)?;
        let elapsed_micros = u64r(r)?;
        let mut interrupted = Vec::new();
        for _ in 0..limited_count(r, 256)? {
            let x = contextr(r)?;
            let stage = u8r(r)?;
            let requested = value_codec::requestedr(r)?;
            let planned = value_codec::kindr(r)?;
            planned.validate()?;
            let moves = u64r(r)?;
            let completed = if boolr(r)? {
                Some(value_codec::kindr(r)?)
            } else {
                None
            };
            let mut b = [0; 2];
            r.read_exact(&mut b)?;
            let score = i16::from_le_bytes(b);
            if (score != 127) != completed.is_some()
                || score != 127 && !(-64..=64).contains(&score)
                || stage as usize >= config.stage_count()
            {
                return Err(invalid("interrupted iteration"));
            }
            let reason = stopr(r)?;
            if reason == GenerationStop::Exhausted {
                return Err(invalid("completed interrupted job"));
            }
            interrupted.push(InterruptedAnalysis {
                root: x.key.board(),
                side: x.side,
                stage,
                requested,
                planned,
                moves,
                completed,
                score: (score != 127).then_some(score),
                reason,
                nodes: u64r(r)?,
            });
        }
        c.runs.push(GenerationRun {
            engine_version,
            search_revision,
            build_label: (!build.is_empty()).then_some(build),
            identity,
            config,
            roots,
            started_unix_seconds,
            threads: threads as u16,
            main_tt_mib,
            limits,
            status,
            nodes,
            elapsed_micros,
            interrupted,
        });
    }
    let mut allowance = limits.max_bytes.saturating_sub(c.bytes());
    for _ in 0..limited_count(r, (allowance / 96) as usize)? {
        let id = idr(r)?;
        let count = u8r(r)?;
        if count == 0 || count > 64 {
            return Err(invalid("derivation arity"));
        }
        let bytes = 96 + u64::from(count) * 32;
        allowance = allowance
            .checked_sub(bytes)
            .ok_or(BookError::Limit("derivation allocation"))?;
        let mut inputs = Vec::with_capacity(count as usize);
        for _ in 0..count {
            inputs.push(idr(r)?);
        }
        if c.derivations.insert(id, inputs).is_some() {
            return Err(invalid("duplicate derivation"));
        }
    }
    if c.bytes() > limits.max_bytes {
        return Err(BookError::Limit("provenance allocation"));
    }
    Ok(c)
}
fn provenance_inputs(c: &catalog::Catalog, value: Assessment) -> Result<Vec<[u8; 32]>> {
    Ok(match value.analysis() {
        ValueAnalysis::Derived {
            basis: Derivation::Maximum { inputs, .. },
            ..
        } => c
            .derivations
            .get(&inputs)
            .ok_or_else(|| invalid("missing derivation input list"))?
            .clone(),
        ValueAnalysis::Derived {
            basis: Derivation::Child { input, .. },
            ..
        } => vec![input],
        ValueAnalysis::Derived {
            basis: Derivation::Intersection { inputs, .. },
            ..
        } => inputs.to_vec(),
        _ => Vec::new(),
    })
}

fn legacy_summary(values: impl IntoIterator<Item = Assessment>) -> QualitySummary {
    let mut q = QualitySummary::empty();
    for v in values {
        let b = v.analysis().summary();
        q.families |= b.families;
        q.horizon = q.horizon.min(b.horizon);
        q.mid_depth = q.mid_depth.min(b.mid_depth);
        q.mid_sel = q.mid_sel.min(b.mid_sel);
        q.end_sel = q.end_sel.min(b.end_sel);
        q.legacy_depth = q.legacy_depth.min(b.legacy_depth);
        q.legacy_sel = q.legacy_sel.min(b.legacy_sel);
    }
    q
}
// 旧playの計算元を検査した後、現在の品質集計で説明を作り直す。
// 手動値の表示深さを旧探索の最低深さへ混ぜていた版も受け入れるが、旧検査は緩めない。
// 入力関係を子から処理するので、複数段の計算でも古い集計結果を引き継がない。
fn rebuild_legacy_play_quality(c: &catalog::Catalog, values: &mut [Assessment]) -> Result<bool> {
    if !values
        .iter()
        .any(|v| matches!(v.analysis(), ValueAnalysis::Derived { .. }))
    {
        return Ok(false);
    }
    let mut indexes = BTreeMap::new();
    for (i, v) in values.iter().enumerate() {
        indexes.insert(v.content_id(), i);
    }
    let mut remaining = vec![0; values.len()];
    let mut parents = vec![Vec::new(); values.len()];
    let mut ready = VecDeque::new();
    for (i, &v) in values.iter().enumerate() {
        let ids = provenance_inputs(c, v)?;
        remaining[i] = ids.len();
        if ids.is_empty() {
            ready.push_back(i);
        }
        for id in ids {
            let input = *indexes
                .get(&id)
                .ok_or_else(|| invalid("missing evidence input"))?;
            parents[input].push(i);
        }
    }
    let mut seen = 0;
    let mut changed = false;
    while let Some(i) = ready.pop_front() {
        seen += 1;
        let value = values[i];
        if let ValueAnalysis::Derived { basis, .. } = value.analysis() {
            let ids = provenance_inputs(c, value)?;
            let mut summary = QualitySummary::empty();
            for id in ids {
                summary.include(values[indexes[&id]].analysis().summary());
            }
            if let Derivation::Child { placement, .. } = basis {
                summary = summary.shifted(placement);
            }
            changed |= summary != value.analysis().summary();
            values[i] = Assessment::new(
                value.claim(),
                ValueAnalysis::Derived { basis, summary },
                value.identity(),
            )?;
        }
        for &parent in &parents[i] {
            remaining[parent] -= 1;
            if remaining[parent] == 0 {
                ready.push_back(parent);
            }
        }
    }
    if seen != values.len() {
        return Err(invalid("cyclic evidence inputs"));
    }
    Ok(changed)
}
// 旧形式の「どの評価から計算したか」という入力関係を、当時の規則で検査する。
// 先にValueStoreへ入れると旧入力IDが消えるため、必ず変換前に呼ぶ。
fn validate_catalog(c: &catalog::Catalog, values: &[Assessment]) -> Result<()> {
    let historical = values.iter().any(|v| {
        matches!(
            v.analysis(),
            ValueAnalysis::Derived {
                basis: Derivation::Maximum { .. }
                    | Derivation::Child { .. }
                    | Derivation::Intersection { .. },
                ..
            }
        )
    });
    if !historical {
        if !c.derivations.is_empty() {
            return Err(invalid("unexpected historical derivations"));
        }
        for &v in values {
            v.validate()?;
            if let ValueAnalysis::Search { trace, .. } = v.analysis()
                && let Some(i) = trace.run
            {
                let run = c
                    .runs
                    .get(i as usize)
                    .ok_or_else(|| invalid("search run reference"))?;
                if run.identity != v.identity() || !c.profiles.contains_key(&v.identity()) {
                    return Err(invalid("search profile reference"));
                }
            }
        }
        return Ok(());
    }
    let map: BTreeMap<_, _> = values.iter().map(|&v| (v.content_id(), v)).collect();
    // Each derivation is checked from its immediate immutable inputs. Exact
    // content IDs bind every link; cycles are rejected by a separate walk.
    for &v in values {
        v.validate()?;
        let ids = provenance_inputs(c, v)?;
        let inputs: Vec<_> = ids
            .iter()
            .map(|id| {
                map.get(id)
                    .copied()
                    .ok_or_else(|| invalid("missing evidence input"))
            })
            .collect::<Result<_>>()?;
        match v.analysis() {
            ValueAnalysis::Search { trace, .. } => {
                if let Some(id) = trace.run {
                    let run = c
                        .runs
                        .get(id as usize)
                        .ok_or_else(|| invalid("search run reference"))?;
                    if run.identity != v.identity() || !c.profiles.contains_key(&v.identity()) {
                        return Err(invalid("search profile reference"));
                    }
                }
            }
            ValueAnalysis::Derived {
                basis: Derivation::Maximum { root, policy, .. },
                ..
            } => {
                let key = Key(root[0], root[1]);
                let actions = legal_actions(key.board());
                if inputs.len() != actions.len() || actions.is_empty() {
                    return Err(invalid("maximum coverage"));
                }
                for (&input, action) in inputs.iter().zip(actions) {
                    input.validate_candidate(key, action)?;
                }
                let policy = match policy {
                    0 => PropagationPolicy::Quality,
                    1 => PropagationPolicy::Book,
                    _ => return Err(invalid("derivation policy")),
                };
                let old_summary = legacy_summary(inputs.iter().copied());
                if combine_values(inputs.into_iter().map(Some), v.identity(), key, policy)
                    .0
                    .map(|a| (a.claim(), old_summary))
                    != Some((v.claim(), v.analysis().summary()))
                {
                    return Err(invalid("derived maximum"));
                }
            }
            ValueAnalysis::Derived {
                basis: Derivation::Child { placement, .. },
                ..
            } => {
                if !inputs[0].whole_position() || {
                    let a = inputs[0].child_to_parent(placement);
                    (a.claim(), a.analysis().summary()) != (v.claim(), v.analysis().summary())
                } {
                    return Err(invalid("derived child backup"));
                }
            }
            ValueAnalysis::Derived {
                basis: Derivation::Intersection { root, action, .. },
                summary,
            } => {
                let key = Key(root[0], root[1]);
                let action = action_from(action)?;
                for &input in &inputs {
                    input.validate_candidate(key, action)?;
                }
                let mut expected = inputs[0].analysis().summary();
                expected.include(inputs[1].analysis().summary());
                let a = inputs[0]
                    .claim()
                    .certificate()
                    .ok_or_else(|| invalid("unproven intersection"))?;
                let b = inputs[1]
                    .claim()
                    .certificate()
                    .ok_or_else(|| invalid("unproven intersection"))?;
                if v.claim() != ValueClaim::Certified(a.intersect(b)?) || summary != expected {
                    return Err(invalid("derived intersection"));
                }
            }
            _ => {}
        }
    }
    // Kahn's algorithm bounds both time and stack usage on untrusted files.
    let mut remaining: BTreeMap<_, usize> = BTreeMap::new();
    let mut parents: BTreeMap<_, Vec<[u8; 32]>> = BTreeMap::new();
    let mut ready = VecDeque::new();
    for &v in values {
        let id = v.content_id();
        let inputs = provenance_inputs(c, v)?;
        if inputs.is_empty() {
            ready.push_back(id);
        }
        remaining.insert(id, inputs.len());
        for input in inputs {
            parents.entry(input).or_default().push(id);
        }
    }
    let mut seen = 0;
    while let Some(id) = ready.pop_front() {
        seen += 1;
        if let Some(ps) = parents.get(&id) {
            for p in ps {
                let n = remaining.get_mut(p).unwrap();
                *n -= 1;
                if *n == 0 {
                    ready.push_back(*p);
                }
            }
        }
    }
    if seen != values.len() {
        return Err(invalid("cyclic evidence"));
    }
    Ok(())
}
// 一時的なAssessment一覧を現行の共有一覧へ変換する。values.rsが探索記録を分離して書く。
fn poolw(w: &mut dyn Write, values: &[Assessment]) -> Result<()> {
    let (store, ids) = ValueStore::from_values(values.iter().copied());
    if ids
        .iter()
        .enumerate()
        .any(|(i, id)| id.get() as usize != i + 1)
    {
        return Err(invalid("noncanonical current values"));
    }
    store.write(w)
}
// Include the pool, its construction index, the validation graph, and the
// editor's retained facts in the decoding allowance; not just file bytes.
const VALUE_READ_BYTES: u64 = 3 * std::mem::size_of::<Assessment>() as u64 + 256;
fn remaining_limits(limits: ReadLimits, used: u64) -> ReadLimits {
    ReadLimits {
        max_bytes: limits.max_bytes.saturating_sub(used),
        ..limits
    }
}
fn poolr(r: &mut impl Read, limits: ReadLimits, modern: bool) -> Result<ValuePool> {
    if modern {
        let store = ValueStore::read(r, limits)?;
        let mut pool = ValuePool::default();
        for (i, v) in store.iter().enumerate() {
            if pool.intern(v) as usize != i {
                return Err(invalid("duplicate value record"));
            }
        }
        return Ok(pool);
    }
    let count = limited_count(
        r,
        limits
            .max_candidates
            .saturating_mul(5)
            .saturating_add(limits.max_positions.saturating_mul(3))
            .min(limits.max_bytes / VALUE_READ_BYTES) as usize,
    )?;
    let mut pool = ValuePool::default();
    reserve(&mut pool.values, count)?;
    for i in 0..count {
        let v = super::value_codec::read(r)?;
        if matches!(
            v.analysis(),
            ValueAnalysis::Derived {
                basis: Derivation::Current { .. },
                ..
            }
        ) {
            return Err(invalid("current calculation in an old-format table"));
        }
        if pool.intern(v) as usize != i {
            return Err(invalid("duplicate value record"));
        }
    }
    Ok(pool)
}
fn refw(w: &mut dyn Write, value: Option<u32>) -> Result<()> {
    u32w(w, value.unwrap_or(u32::MAX))
}
fn refr(r: &mut impl Read, values: &[Assessment]) -> Result<Option<u32>> {
    let id = u32r(r)?;
    if id == u32::MAX {
        Ok(None)
    } else if (id as usize) < values.len() {
        Ok(Some(id))
    } else {
        Err(invalid("value reference"))
    }
}
fn evidencew(w: &mut dyn Write, value: Option<Assessment>, pool: &ValuePool) -> Result<()> {
    refw(w, value.map(|v| pool.id(v)))
}
fn evidencer(
    r: &mut impl Read,
    id: AnalysisIdentity,
    latest: bool,
    pool: &ValuePool,
) -> Result<Option<Assessment>> {
    if latest {
        Ok(refr(r, &pool.values)?.map(|i| pool.values[i as usize]))
    } else {
        valuer(r, id, true)
    }
}
fn valuer(r: &mut impl Read, id: AnalysisIdentity, full: bool) -> Result<Option<Assessment>> {
    if !boolr(r)? {
        return Ok(None);
    }
    let mut b = [0; 2];
    r.read_exact(&mut b)?;
    let depth = u8r(r)?;
    let sel = selectivity(u8r(r)?)?;
    let solved = boolr(r)?;
    let bound = match u8r(r)? {
        0 => Bound::Exact,
        1 => Bound::Lower,
        2 => Bound::Upper,
        3 => Bound::ProvenLower,
        4 => Bound::ProvenUpper,
        _ => return Err(invalid("bound")),
    };
    let origin = match u8r(r)? {
        0 => ValueOrigin::Search,
        1 => ValueOrigin::Manual,
        2 => ValueOrigin::Imported,
        3 => ValueOrigin::Rules,
        _ => return Err(invalid("origin")),
    };
    let v = Assessment::import(ImportedValue {
        score: i16::from_le_bytes(b),
        quality: AnalysisQuality {
            depth,
            selectivity: sel,
            solved,
        },
        bound,
        origin,
        identity: if full { idr(r)? } else { id },
    })?;
    v.validate()?;
    Ok(Some(v))
}
fn counts(r: &mut impl Read, limits: ReadLimits) -> Result<(usize, usize)> {
    let n = u64r(r)?;
    let e = u64r(r)?;
    if n > limits.max_positions || e > limits.max_candidates || e > n.saturating_mul(64) {
        return Err(BookError::Limit("record counts"));
    }
    Ok((
        usize::try_from(n).map_err(|_| BookError::Limit("positions"))?,
        usize::try_from(e).map_err(|_| BookError::Limit("candidates"))?,
    ))
}
fn reserve<T>(v: &mut Vec<T>, n: usize) -> Result<()> {
    v.try_reserve_exact(n)
        .map_err(|_| BookError::Limit("allocation"))
}
pub(super) fn save_book(book: &Book, path: &Path) -> Result<()> {
    atomic_save(path, BOOK, 4, |w| {
        w.write_all(&book.identity)?;
        u8w(w, book.propagation as u8)?;
        stringw(w, &book.producer)?;
        u64w(w, book.records.len() as u64)?;
        u64w(w, book.candidates.len() as u64)?;
        poolw(w, &book.values.iter().collect::<Vec<_>>())?;
        catalogw(w, &book.catalog)?;
        for record in &book.records {
            keyw(w, record.key)?;
            u8w(w, record.len as u8)?;
            u8w(w, u8::from(record.complete))?;
            refw(w, record.value)?;
            refw(w, record.proof)?;
            for c in &book.candidates[record.start..record.start + record.len] {
                u8w(w, action_code(c.action))?;
                u8w(w, u8::from(c.registered))?;
                refw(w, c.value)?;
                refw(w, c.certificate)?;
            }
        }
        extensionsw(w, &book.extensions)
    })
}
pub(super) fn load_book(path: &Path, limits: ReadLimits) -> Result<Book> {
    let (mut input, version) = open(path, BOOK, limits)?;
    let identity = idr(&mut input)?;
    let propagation = if version >= 3 {
        match u8r(&mut input)? {
            0 => PropagationPolicy::Quality,
            1 => PropagationPolicy::Book,
            _ => return Err(invalid("propagation policy")),
        }
    } else {
        PropagationPolicy::Quality
    };
    let producer = stringr(&mut input)?;
    let (n, total) = counts(&mut input, limits)?;
    let resident = (n as u64).saturating_mul(128).saturating_add(
        (total as u64).saturating_mul(std::mem::size_of::<StoredCandidate>() as u64),
    );
    if resident > limits.max_bytes {
        return Err(BookError::Limit("resident allocation"));
    }
    let mut pool = if version >= 3 {
        poolr(&mut input, remaining_limits(limits, resident), version >= 4)?
    } else {
        ValuePool::default()
    };
    let mut catalog = if version >= 3 {
        catalogr(
            &mut input,
            remaining_limits(
                limits,
                resident + pool.values.len() as u64 * VALUE_READ_BYTES,
            ),
            version >= 4,
        )?
    } else {
        catalog::Catalog::default()
    };
    validate_catalog(&catalog, &pool.values)?;
    let mut records = Vec::new();
    let mut candidates = Vec::new();
    reserve(&mut records, n)?;
    reserve(&mut candidates, total)?;
    for _ in 0..n {
        let key = keyr(&mut input)?;
        if records.last().is_some_and(|p: &Record| p.key >= key) {
            return Err(invalid("duplicate or unsorted position"));
        }
        let len = u8r(&mut input)? as usize;
        let legal = legal_actions(key.board());
        if len != legal.len() || candidates.len().saturating_add(len) > total {
            return Err(invalid("candidate count"));
        }
        let complete = boolr(&mut input)?;
        let value = if version >= 3 {
            refr(&mut input, &pool.values)?
        } else {
            valuer(&mut input, identity, false)?.map(|v| pool.intern(v))
        };
        let proof = if version >= 3 {
            refr(&mut input, &pool.values)?
        } else {
            None
        };
        let start = candidates.len();
        for legal in legal {
            let action = action_from(u8r(&mut input)?)?;
            if action != legal {
                return Err(invalid("illegal or duplicate candidate"));
            }
            let registered = boolr(&mut input)?;
            let value = if version >= 3 {
                refr(&mut input, &pool.values)?
            } else {
                valuer(&mut input, identity, false)?.map(|v| pool.intern(v))
            };
            if let Some(v) = value {
                pool.values[v as usize].validate_candidate(key, action)?;
            }
            candidates.push(StoredCandidate {
                action,
                registered,
                certificate: if version >= 3 {
                    refr(&mut input, &pool.values)?
                } else {
                    value.filter(|i| pool.values[*i as usize].proven())
                },
                value,
            });
        }
        records.push(Record {
            key,
            start,
            len,
            complete,
            value,
            proof,
        });
    }
    if candidates.len() != total {
        return Err(invalid("candidate total"));
    }
    if version < 3 {
        for rec in &mut records {
            let entries = &candidates[rec.start..rec.start + rec.len];
            let effective: Vec<_> = entries
                .iter()
                .map(|c| c.value.map(|i| pool.values[i as usize]))
                .collect();
            let certificates: Vec<_> = entries
                .iter()
                .map(|c| c.certificate.map(|i| pool.values[i as usize]))
                .collect();
            let expected = if entries.is_empty() {
                Some(terminal_value(rec.key.board()))
            } else {
                combine_values(effective.iter().copied(), identity, rec.key, propagation).0
            };
            let old = rec.value.map(|i| pool.values[i as usize]);
            let view = |v: Assessment| (v.score(), v.quality(), v.bound());
            if expected.map(view) != old.map(view)
                && !(old.is_none() && expected.is_some_and(|v| v.bound() != Bound::Exact))
            {
                return Err(invalid("legacy position summary"));
            }
            let proof = if entries.is_empty() {
                Some(terminal_value(rec.key.board()))
            } else {
                combine_values(certificates.iter().copied(), identity, rec.key, propagation).0
            };
            for (value, inputs) in [(expected, &effective), (proof, &certificates)] {
                if let Some(v) = value
                    && let ValueAnalysis::Derived {
                        basis: Derivation::Maximum { inputs: digest, .. },
                        ..
                    } = v.analysis()
                {
                    catalog.derivations.insert(
                        digest,
                        inputs.iter().flatten().map(|v| v.content_id()).collect(),
                    );
                }
            }
            rec.value = expected.map(|v| pool.intern(v));
            rec.proof = proof.map(|v| pool.intern(v));
        }
    }
    validate_catalog(&catalog, &pool.values)?;
    let used = resident + pool.values.len() as u64 * VALUE_READ_BYTES + catalog.bytes();
    if used > limits.max_bytes {
        return Err(BookError::Limit("play decoding memory"));
    }
    let mut extensions = extensionsr(&mut input, limits.max_bytes.saturating_sub(used))?;
    done(&mut input)?;
    if version == 3 {
        // poolの検索索引はこの先使わず、valuesだけをBookへ渡す。
        if rebuild_legacy_play_quality(&catalog, &mut pool.values)? {
            // 旧品質を前提にした任意拡張の意味は保証できない。
            extensions.clear();
        }
    }
    let book = Book::from_parts(
        SnapshotMetadata {
            identity,
            propagation,
            catalog,
            producer,
        },
        pool.values,
        extensions,
        records,
        candidates,
    )?;
    for rec in &book.records {
        let entries = &book.candidates[rec.start..rec.start + rec.len];
        for c in entries.iter().filter(|c| c.registered) {
            if book
                .probe(rec.key.board().play(c.action).unwrap())
                .is_none()
            {
                return Err(invalid("dangling link"));
            }
        }
        for c in entries {
            if let Some(i) = c.certificate {
                let v = book.values.at(i as usize);
                v.validate_candidate(rec.key, c.action)?;
                if !v.proven() {
                    return Err(invalid("unproven candidate certificate"));
                }
            }
        }
        let proof = if entries.is_empty() {
            Some(terminal_value(rec.key.board()))
        } else {
            combine_values(
                entries
                    .iter()
                    .map(|c| c.certificate.map(|i| book.values.at(i as usize))),
                identity,
                rec.key,
                book.propagation,
            )
            .0
        };
        if rec.proof.map(|i| book.values.at(i as usize)) != proof {
            return Err(invalid("position proof summary"));
        }
        let all = entries.iter().all(|c| {
            c.value
                .is_some_and(|v| book.values.at(v as usize).bound() == Bound::Exact)
        });
        if rec.complete != all {
            return Err(invalid("coverage summary"));
        }
        let expected = if entries.is_empty() {
            Some(terminal_value(rec.key.board()))
        } else {
            combine_values(
                entries
                    .iter()
                    .map(|c| c.value.map(|v| book.values.at(v as usize))),
                identity,
                rec.key,
                book.propagation,
            )
            .0
        };
        let actual = rec.value.map(|v| book.values.at(v as usize));
        if if version >= 3 {
            expected != actual
        } else {
            expected.map(|v| (v.score(), v.quality(), v.bound()))
                != actual.map(|v| (v.score(), v.quality(), v.bound()))
        } {
            return Err(invalid("position value summary"));
        }
    }
    Ok(book)
}
// workは直接評価・手動値・確定情報・集合探索・仕事一覧を保存する。
// Node.value/backups、parents、優先順位などの補助データは読込後に再計算する。
pub(super) fn save_editor(editor: &BookEditor, path: &Path) -> Result<()> {
    editor.validate()?;
    let pool = ValuePool::editor(editor);
    atomic_save(path, WORK, 8, |w| {
        w.write_all(&editor.identity)?;
        u8w(w, editor.propagation as u8)?;
        stringw(w, &editor.producer)?;
        u64w(w, editor.revision)?;
        u64w(w, editor.nodes.len() as u64)?;
        u64w(w, editor.nodes.values().map(|n| n.edges.len() as u64).sum())?;
        poolw(w, &pool.values)?;
        catalogw(w, &editor.catalog)?;
        for (&key, n) in &editor.nodes {
            keyw(w, key)?;
            u64w(w, n.revision)?;
            u8w(w, n.edges.len() as u8)?;
            u64w(w, n.completed_stages as u64)?;
            for e in &n.edges {
                u8w(w, action_code(e.action))?;
                u8w(w, u8::from(e.registered))?;
                evidencew(w, editor.values.opt(e.direct()), &pool)?;
                evidencew(w, editor.values.opt(e.manual()), &pool)?;
                evidencew(w, editor.values.opt(e.certificate()), &pool)?;
            }
            u8w(w, u8::from(n.pending.is_some()))?;
            if let Some(p) = &n.pending {
                u64w(w, p.stage as u64)?;
                u64w(w, p.revision)?;
                for &v in &p.values {
                    evidencew(w, editor.values.opt(v), &pool)?;
                }
            }
            u8w(w, u8::from(n.frontier.is_some()))?;
            if let Some(f) = &n.frontier {
                u64w(w, f.mask)?;
                u8w(w, action_code(f.best))?;
                evidencew(w, Some(editor.values.get(f.value)), &pool)?;
                u8w(w, f.pv.len() as u8)?;
                for &a in &f.pv {
                    u8w(w, action_code(a))?;
                }
            }
        }
        u8w(w, u8::from(editor.generation.is_some()))?;
        if let Some(g) = &editor.generation {
            configw(w, &g.config)?;
            u64w(w, g.iteration)?;
            u64w(w, g.roots.len() as u64)?;
            for &k in &g.roots {
                contextw(w, k)?;
            }
            u64w(w, g.distances.len() as u64)?;
            for (&k, &d) in &g.distances {
                contextw(w, k)?;
                u8w(w, d)?;
            }
            u64w(w, g.fresh.len() as u64)?;
            for &(d, k) in &g.fresh {
                u8w(w, d)?;
                contextw(w, k)?;
            }
            u64w(w, g.deepen.len() as u64)?;
            for &(s, d, k) in &g.deepen {
                u64w(w, s as u64)?;
                u8w(w, d)?;
                contextw(w, k)?;
            }
            u64w(w, g.expansions.len() as u64)?;
            for &(d, k) in &g.expansions {
                u8w(w, d)?;
                contextw(w, k)?;
            }
            u64w(w, g.urgent.len() as u64)?;
            for &(c, d, stage, left) in &g.urgent {
                contextw(w, c)?;
                u8w(w, d)?;
                u64w(w, stage as u64)?;
                u8w(w, left)?;
            }
            u64w(w, g.hints.len() as u64)?;
            for (&c, &a) in &g.hints {
                contextw(w, c)?;
                u8w(w, action_code(a))?;
            }
        }
        extensionsw(w, &editor.extensions)
    })
}
fn limited_count(r: &mut impl Read, max: usize) -> Result<usize> {
    let n = u64r(r)?;
    if n > max as u64 {
        return Err(BookError::Limit("work count"));
    }
    Ok(n as usize)
}
type ComparedValue = (ValueClaim, QualitySummary, ValueOrigin);
fn compared(v: Option<Assessment>) -> Option<ComparedValue> {
    v.map(|v| (v.claim(), v.analysis().summary(), v.origin()))
}
// This adapter is only for old-file conversion reports. New editing and
// generation have one child-first rule and never call the old quality choice.
fn legacy_values(
    editor: &BookEditor,
    policy: PropagationPolicy,
) -> BTreeMap<Key, (Option<Assessment>, Vec<Option<ComparedValue>>)> {
    let mut cache: BTreeMap<Key, (Option<Assessment>, Vec<Option<ComparedValue>>)> =
        BTreeMap::new();
    let mut keys: Vec<_> = editor.nodes.keys().copied().map(editor::order).collect();
    keys.sort_unstable();
    for (_, _, key) in keys {
        let node = &editor.nodes[&key];
        let mut inputs = Vec::with_capacity(node.edges.len());
        for (index, e) in node.edges.iter().enumerate() {
            let direct = editor.values.opt(e.direct());
            let manual = editor
                .values
                .opt(e.manual())
                .filter(|v| v.identity() == editor.identity);
            let child = if e.registered {
                cache
                    .get(&e.child)
                    .and_then(|v| v.0)
                    .map(|v| v.child_to_parent(matches!(e.action, Action::Place(_))))
            } else {
                None
            };
            let (mut value, mut backed) = if let Some(v) = manual {
                (Some(v), false)
            } else {
                match (direct, child) {
                    (Some(d), _) if d.quality().solved => (Some(d), false),
                    (Some(d), Some(p))
                        if (!d.proven() || p.proven())
                            && (policy == PropagationPolicy::Book || editor::dominates(p, d)) =>
                    {
                        (Some(p), true)
                    }
                    (Some(d), _) => (Some(d), false),
                    (None, Some(p)) => (Some(p), true),
                    (None, None) => (None, false),
                }
            };
            if manual.is_none() {
                if let Some(f) = node.frontier.as_ref()
                    && let Some(v) = f.assessment(index, e.action, &editor.values)
                    && !(policy == PropagationPolicy::Book && backed && !v.proven())
                    && value.is_none_or(|d| {
                        if d.quality().solved {
                            false
                        } else if d.proven() {
                            v.proven()
                                && (editor::dominates(v, d)
                                    || (v.bound() == Bound::ProvenUpper
                                        && d.bound() == Bound::ProvenLower))
                        } else {
                            editor::dominates(editor.values.get(f.value), d)
                        }
                    })
                {
                    value = Some(v);
                    backed = false;
                }
                if let Some(v) = editor.values.opt(e.certificate()) {
                    value = Some(v);
                }
            }
            let _ = backed;
            inputs.push(value);
        }
        let value = if node.edges.is_empty() {
            Some(terminal_value(key.board()))
        } else {
            combine_values(inputs.iter().copied(), editor.identity, key, policy).0
        };
        let value = value.map(|v| {
            if let ValueAnalysis::Derived { basis, .. } = v.analysis() {
                Assessment::new(
                    v.claim(),
                    ValueAnalysis::Derived {
                        basis,
                        summary: legacy_summary(inputs.iter().flatten().copied()),
                    },
                    v.identity(),
                )
                .expect("validated legacy summary")
            } else {
                v
            }
        });
        cache.insert(key, (value, inputs.into_iter().map(compared).collect()));
    }
    cache
}
// 読込→旧情報の検査→現行記録へ変換→repair→旧途中状態の取込→移行差分の確認。
// この関数は元ファイルを書き換えない。変換結果の永続化は呼出し側のsaveで行う。
pub(super) fn load_editor(path: &Path, limits: ReadLimits) -> Result<BookEditor> {
    let (mut input, version) = open(path, WORK, limits)?;
    let identity = idr(&mut input)?;
    let propagation = if version >= 4 {
        match u8r(&mut input)? {
            0 => PropagationPolicy::Quality,
            1 => PropagationPolicy::Book,
            _ => return Err(invalid("propagation policy")),
        }
    } else {
        PropagationPolicy::Quality
    };
    let producer = stringr(&mut input)?;
    let revision = u64r(&mut input)?;
    let (n, total) = counts(&mut input, limits)?;
    let resident = (n as u64)
        .saturating_mul(std::mem::size_of::<Node>() as u64 + 768)
        .saturating_add((total as u64).saturating_mul(
            (std::mem::size_of::<Edge>() + std::mem::size_of::<Option<ValueId>>()) as u64,
        ));
    if resident > limits.max_bytes {
        return Err(BookError::Limit("workspace allocation"));
    }
    let pool = if version >= 7 {
        poolr(&mut input, remaining_limits(limits, resident), version >= 8)?
    } else {
        ValuePool::default()
    };
    let catalog = if version >= 7 {
        catalogr(
            &mut input,
            remaining_limits(
                limits,
                resident + pool.values.len() as u64 * VALUE_READ_BYTES,
            ),
            version >= 8,
        )?
    } else {
        catalog::Catalog::default()
    };
    validate_catalog(&catalog, &pool.values)?;
    let mut editor = BookEditor::new(identity);
    editor.catalog = catalog;
    editor.catalog.derivations.clear();
    editor.producer = producer;
    let _old_propagation = propagation;
    editor.propagation = PropagationPolicy::Book;
    let mut count = 0;
    for _ in 0..n {
        let key = keyr(&mut input)?;
        if editor.nodes.contains_key(&key) {
            return Err(invalid("duplicate position"));
        }
        let node_revision = u64r(&mut input)?;
        if node_revision > revision {
            return Err(invalid("node revision"));
        }
        let len = u8r(&mut input)? as usize;
        let stages = limited_count(&mut input, 60)?;
        let legal = legal_actions(key.board());
        if len != legal.len() || count + len > total {
            return Err(invalid("candidate count"));
        }
        count += len;
        let mut edges = Vec::new();
        reserve(&mut edges, len)?;
        for a in legal {
            let action = action_from(u8r(&mut input)?)?;
            if action != a {
                return Err(invalid("candidate order"));
            }
            let registered = boolr(&mut input)?;
            let direct = editor
                .values
                .put(evidencer(&mut input, identity, version >= 7, &pool)?);
            let manual = editor
                .values
                .put(evidencer(&mut input, identity, version >= 7, &pool)?);
            let certificate = if version >= 7 {
                editor
                    .values
                    .put(evidencer(&mut input, identity, true, &pool)?)
            } else {
                None
            };
            edges.push(Edge::new(
                action,
                Key::new(key.board().play(action).unwrap()).0,
                registered,
                direct,
                manual,
                certificate,
            ));
        }
        let pending = if boolr(&mut input)? {
            if version >= 8 {
                return Err(invalid("legacy pending results are not allowed in work v8"));
            }
            let stage = limited_count(&mut input, 59)?;
            let revision = u64r(&mut input)?;
            let mut values = Vec::new();
            reserve(&mut values, len)?;
            for _ in 0..len {
                values.push(editor.values.put(evidencer(
                    &mut input,
                    identity,
                    version >= 7,
                    &pool,
                )?));
            }
            Some(Pending {
                stage,
                revision,
                values,
            })
        } else {
            None
        };
        let frontier = if version >= 6 && boolr(&mut input)? {
            let mask = u64r(&mut input)?;
            let best = action_from(u8r(&mut input)?)?;
            let value = evidencer(&mut input, identity, version >= 7, &pool)?
                .ok_or_else(|| invalid("missing frontier value"))?;
            let length = u8r(&mut input)? as usize;
            if length > 120 {
                return Err(invalid("frontier PV length"));
            }
            let mut pv = Vec::with_capacity(length);
            for _ in 0..length {
                pv.push(action_from(u8r(&mut input)?)?);
            }
            Some(Frontier::new(mask, best, value, pv, &mut editor.values)?)
        } else {
            None
        };
        editor.ensure(key)?;
        let n = editor.nodes.get_mut(&key).unwrap();
        *n = Node {
            revision: node_revision,
            notes_bytes: 0,
            edges,
            value: None,
            complete: false,
            backups: [None; 2],
            proof: None,
            proof_backups: None,
            completed_stages: stages,
            pending,
            frontier,
        };
        editor.pending_count += usize::from(n.pending.is_some());
    }
    if count != total {
        return Err(invalid("candidate total"));
    }
    editor.revision = revision;
    if boolr(&mut input)? {
        let config = configr(&mut input, version)?;
        let iteration = u64r(&mut input)?;
        let mut queue_budget = limits.max_bytes.saturating_sub(
            resident + pool.values.len() as u64 * VALUE_READ_BYTES + editor.catalog.bytes(),
        );
        let nr = queued_count(&mut input, n.saturating_mul(2), &mut queue_budget)?;
        if nr == 0 {
            return Err(invalid("empty roots"));
        }
        let mut roots = Vec::new();
        reserve(&mut roots, nr)?;
        for _ in 0..nr {
            let k = contextr(&mut input)?;
            if !editor.nodes.contains_key(&k.key) || roots.last().is_some_and(|p| *p >= k) {
                return Err(invalid("root"));
            }
            roots.push(k);
        }
        let mut distances = BTreeMap::new();
        for _ in 0..queued_count(&mut input, n.saturating_mul(2), &mut queue_budget)? {
            let k = contextr(&mut input)?;
            let d = u8r(&mut input)?;
            if d > config.max_plies
                || !editor.nodes.contains_key(&k.key)
                || distances.insert(k, d).is_some()
            {
                return Err(invalid("distance"));
            }
        }
        for k in &roots {
            if distances.get(k) != Some(&0) {
                return Err(invalid("root distance"));
            }
        }
        let mut fresh = BTreeSet::new();
        for _ in 0..queued_count(&mut input, n.saturating_mul(122), &mut queue_budget)? {
            let d = u8r(&mut input)?;
            let k = contextr(&mut input)?;
            if d > config.max_plies
                || distances.get(&k).is_none_or(|old| *old > d)
                || !fresh.insert((d, k))
            {
                return Err(invalid("fresh queue"));
            }
        }
        let mut deepen = BTreeSet::new();
        for _ in 0..queued_count(&mut input, n.saturating_mul(7320), &mut queue_budget)? {
            let s = limited_count(&mut input, config.stage_count().saturating_sub(1))?;
            let d = u8r(&mut input)?;
            let k = contextr(&mut input)?;
            if d > config.max_plies
                || s == 0
                || distances.get(&k).is_none_or(|old| *old > d)
                || !deepen.insert((s, d, k))
            {
                return Err(invalid("deepening queue"));
            }
        }
        let mut expansions = BTreeSet::new();
        for _ in 0..queued_count(&mut input, n.saturating_mul(122), &mut queue_budget)? {
            let d = u8r(&mut input)?;
            let k = contextr(&mut input)?;
            if d > config.max_plies
                || distances.get(&k).is_none_or(|old| *old > d)
                || !expansions.insert((d, k))
            {
                return Err(invalid("expansion queue"));
            }
        }
        for n in editor.nodes.values() {
            if n.completed_stages > config.stage_count()
                || n.pending.as_ref().is_some_and(|p| {
                    p.stage != n.completed_stages
                        || p.stage >= config.stage_count()
                        || (config.frontier_only
                            && n.edges
                                .iter()
                                .zip(&p.values)
                                .any(|(e, v)| e.registered && v.is_some()))
                })
            {
                return Err(invalid("stage index"));
            }
        }
        let mut urgent = VecDeque::new();
        let mut hints = BTreeMap::new();
        if version >= 6 {
            for _ in 0..queued_count(&mut input, n.saturating_mul(122), &mut queue_budget)? {
                let c = contextr(&mut input)?;
                let d = u8r(&mut input)?;
                let stage = limited_count(&mut input, config.stage_count())?;
                let left = u8r(&mut input)?;
                if !distances.contains_key(&c)
                    || d > config.max_plies
                    || stage >= config.stage_count()
                    || left > config.line_plies
                {
                    return Err(invalid("PV job"));
                }
                urgent.push_back((c, d, stage, left));
            }
            for _ in 0..queued_count(&mut input, n.saturating_mul(2), &mut queue_budget)? {
                let c = contextr(&mut input)?;
                let a = action_from(u8r(&mut input)?)?;
                if !distances.contains_key(&c)
                    || c.key.board().play(a).is_err()
                    || hints.insert(c, a).is_some()
                {
                    return Err(invalid("PV hint"));
                }
            }
        }
        editor.generation = Some(GenerationState {
            reachability: Default::default(),
            config,
            urgent,
            hints,
            active_burst: 0,
            priority: None,
            roots,
            iteration,
            distances,
            fresh,
            deepen,
            expansions,
            expanded: BTreeMap::new(),
        });
    } else if editor.nodes.values().any(|n| n.pending.is_some()) {
        return Err(invalid("pending analysis without generation settings"));
    }
    let used = resident
        + pool.values.len() as u64 * VALUE_READ_BYTES
        + editor.catalog.bytes()
        + editor
            .generation
            .as_ref()
            .map_or(0, |g| g.estimated_bytes());
    if used > limits.max_bytes {
        return Err(BookError::Limit("workspace decoding memory"));
    }
    editor.extensions = extensionsr(&mut input, limits.max_bytes - used)?;
    done(&mut input)?;
    // 旧採用規則で見えていた値を比較用に保持する。旧BookEditor全体の複製ではない。
    let previous = if version < 8 {
        Some(legacy_values(&editor, propagation))
    } else {
        None
    };
    editor.repair()?;
    editor.normalize_if_pending()?;
    if let Some(previous) = previous {
        let mut changed_position_values = 0;
        let mut changed_candidate_values = 0;
        for (&key, node) in &editor.nodes {
            let (old, candidates) = &previous[&key];
            changed_position_values +=
                u64::from(compared(*old) != compared(editor.values.opt(node.value)));
            for (e, old) in node.edges.iter().zip(candidates) {
                changed_candidate_values += u64::from(compared(editor.effective(key, e)) != *old);
            }
        }
        let changes_rules = propagation == PropagationPolicy::Quality
            || changed_position_values > 0
            || changed_candidate_values > 0;
        let mut jobs_rebuilt = false;
        if changes_rules {
            // Old quality-based queues can omit a branch that becomes useful
            // under child-first values. Rebuild work, retaining completed data.
            editor.extensions.clear();
            if let Some(g) = editor.generation.take() {
                let roots: Vec<_> = g
                    .roots
                    .iter()
                    .map(|c| GenerationRoot {
                        board: c.key.board(),
                        side: c.side,
                    })
                    .collect();
                editor.configure_roots_checked(&roots, g.config, false)?;
                jobs_rebuilt = true;
            }
        }
        editor.migration = Some(MigrationReport {
            source_version: version,
            positions: editor.nodes.len() as u64,
            registered_moves: editor.registered_links,
            changed_position_values,
            changed_candidate_values,
            jobs_rebuilt,
        });
    }
    if editor.estimated_bytes() > limits.max_bytes {
        return Err(BookError::Limit("workspace memory"));
    }
    Ok(editor)
}
fn configw(w: &mut dyn Write, c: &GenerationConfig) -> Result<()> {
    u8w(w, u8::from(c.reuse_completed))?;
    match &c.analysis {
        AnalysisSchedule::Stages(v) => {
            u8w(w, 0)?;
            u8w(w, v.len() as u8)?;
            for s in v {
                u8w(w, s.depth)?;
                u8w(w, s.selectivity as u8)?;
            }
        }
        AnalysisSchedule::Levels(v) => {
            u8w(w, 1)?;
            u8w(w, v.len() as u8)?;
            for &n in v {
                u8w(w, n)?;
            }
        }
    }
    u8w(w, c.new_job_weight)?;
    u8w(w, c.deepen_job_weight)?;
    u8w(w, c.black_loss.unwrap_or(255))?;
    u8w(w, c.white_loss.unwrap_or(255))?;
    u8w(w, c.expansion_loss)?;
    u8w(w, c.max_plies)?;
    u64w(w, c.max_memory_bytes)?;
    u64w(w, c.checkpoint_interval.as_secs())?;
    u32w(w, c.checkpoint_interval.subsec_nanos())?;
    u32w(w, c.cumulative_loss.map_or(u32::MAX, u32::from))?;
    u8w(w, c.leaf_loss.unwrap_or(255))?;
    u8w(w, u8::from(c.frontier_only))?;
    u8w(w, c.schedule as u8)?;
    u8w(w, c.line_plies)?;
    u32w(w, u32::from(c.max_parallel_jobs))?;
    u8w(
        w,
        match c.legacy {
            LegacyPolicy::Reject => 0,
            LegacyPolicy::RecordedQuality => 1,
        },
    )
}
fn configr(r: &mut impl Read, version: u32) -> Result<GenerationConfig> {
    let reuse_completed = boolr(r)?;
    let analysis = if version >= 8 {
        let tag = u8r(r)?;
        let n = u8r(r)?;
        if n == 0 || n > 60 {
            return Err(invalid("analysis count"));
        }
        match tag {
            0 => {
                let mut v = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    v.push(Stage {
                        depth: u8r(r)?,
                        selectivity: selectivity(u8r(r)?)?,
                    });
                }
                AnalysisSchedule::Stages(v)
            }
            1 => {
                let mut v = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    v.push(u8r(r)?);
                }
                AnalysisSchedule::Levels(v)
            }
            _ => return Err(invalid("analysis schedule")),
        }
    } else {
        let n = u8r(r)?;
        if n == 0 || n > 60 {
            return Err(invalid("stage count"));
        }
        let mut stages = Vec::new();
        for _ in 0..n {
            stages.push(Stage {
                depth: u8r(r)?,
                selectivity: selectivity(u8r(r)?)?,
            });
        }
        let levels = if version >= 2 {
            let count = u8r(r)?;
            if count > 60 {
                return Err(invalid("level count"));
            }
            if count == 0 {
                None
            } else {
                let mut levels = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    levels.push(u8r(r)?);
                }
                Some(levels)
            }
        } else {
            None
        };
        levels.map_or_else(
            || AnalysisSchedule::Stages(stages),
            AnalysisSchedule::Levels,
        )
    };
    let new_job_weight = u8r(r)?;
    let deepen_job_weight = u8r(r)?;
    let black = u8r(r)?;
    let white = u8r(r)?;
    let black_loss = (black != 255).then_some(black);
    let white_loss = (white != 255).then_some(white);
    let expansion_loss = u8r(r)?;
    let max_plies = u8r(r)?;
    let max_memory_bytes = u64r(r)?;
    let seconds = u64r(r)?;
    let nanos = u32r(r)?;
    if nanos >= 1_000_000_000 {
        return Err(invalid("duration"));
    }
    let (cumulative_loss, leaf_loss) = if version >= 3 {
        let total = u32r(r)?;
        let leaf = u8r(r)?;
        (
            if total == u32::MAX {
                None
            } else {
                Some(u16::try_from(total).map_err(|_| invalid("cumulative loss"))?)
            },
            (leaf != 255).then_some(leaf),
        )
    } else {
        (None, None)
    };
    let frontier_only = version >= 4 && boolr(r)?;
    let schedule = if version >= 5 {
        match u8r(r)? {
            0 => SchedulePolicy::Breadth,
            1 => SchedulePolicy::Frontier,
            _ => return Err(invalid("schedule policy")),
        }
    } else {
        SchedulePolicy::Breadth
    };
    let (line_plies, max_parallel_jobs) = if version >= 6 {
        (
            u8r(r)?,
            u16::try_from(u32r(r)?).map_err(|_| invalid("parallel job limit"))?,
        )
    } else {
        (8, 0)
    };
    let legacy = if version >= 7 {
        match u8r(r)? {
            0 => LegacyPolicy::Reject,
            1 => LegacyPolicy::RecordedQuality,
            _ => return Err(invalid("legacy reuse policy")),
        }
    } else {
        LegacyPolicy::RecordedQuality
    };
    let c = GenerationConfig {
        legacy,
        line_plies,
        max_parallel_jobs,
        reuse_completed,
        frontier_only,
        schedule,
        analysis,
        expansion_loss,
        black_loss,
        white_loss,
        cumulative_loss,
        leaf_loss,
        new_job_weight,
        deepen_job_weight,
        max_plies,
        max_memory_bytes,
        checkpoint_interval: Duration::new(seconds, nanos),
    };
    c.validate()?;
    Ok(c)
}

fn stringw(w: &mut dyn Write, s: &str) -> Result<()> {
    if s.len() > 4096 {
        return Err(invalid("producer length"));
    }
    u32w(w, s.len() as u32)?;
    w.write_all(s.as_bytes())?;
    Ok(())
}
fn stringr(r: &mut impl Read) -> Result<String> {
    let n = u32r(r)? as usize;
    if n > 4096 {
        return Err(invalid("producer length"));
    }
    let mut b = vec![0; n];
    r.read_exact(&mut b)?;
    String::from_utf8(b).map_err(|_| invalid("producer encoding"))
}

fn extensionsw(w: &mut dyn Write, es: &[Extension]) -> Result<()> {
    if es.len() > 1024 {
        return Err(invalid("too many extension sections"));
    }
    u32w(w, es.len() as u32)?;
    for e in es {
        u32w(w, e.kind)?;
        u64w(w, e.bytes.len() as u64)?;
        w.write_all(&e.bytes)?;
    }
    Ok(())
}
// 未知の必須項目は拒否する。未知の任意項目はそのまま残すが、意味のある編集や
// 採用規則の移行が入った場合は、もはや有効とは言えないのでeditor側で消去する。
fn extensionsr(r: &mut impl Read, mut budget: u64) -> Result<Vec<Extension>> {
    let count = u32r(r)?;
    if count > 1024 {
        return Err(invalid("too many extension sections"));
    }
    let mut sections = Vec::new();
    for _ in 0..count {
        let kind = u32r(r)?;
        let len = u64r(r)?;
        if kind & 0x8000_0000 != 0 {
            return Err(invalid("unsupported mandatory extension"));
        }
        if sections.iter().any(|e: &Extension| e.kind == kind) {
            return Err(invalid("duplicate extension"));
        }
        let charge = len
            .checked_add(64)
            .ok_or(BookError::Limit("extension size"))?;
        if charge > budget {
            return Err(BookError::Limit("extension allocation"));
        }
        budget -= charge;
        let n = usize::try_from(len).map_err(|_| BookError::Limit("extension size"))?;
        let mut bytes = Vec::new();
        reserve(&mut bytes, n)?;
        bytes.resize(n, 0);
        r.read_exact(&mut bytes)?;
        sections.push(Extension { kind, bytes });
    }
    Ok(sections)
}

fn queued_count(r: &mut impl Read, max: usize, budget: &mut u64) -> Result<usize> {
    let n = limited_count(r, max)?;
    let bytes = (n as u64)
        .checked_mul(128)
        .ok_or(BookError::Limit("queue allocation"))?;
    if bytes > *budget {
        return Err(BookError::Limit("queue allocation"));
    }
    *budget -= bytes;
    Ok(n)
}
fn contextw(w: &mut dyn Write, c: Context) -> Result<()> {
    keyw(w, c.key)?;
    u8w(w, c.side as u8)
}
fn contextr(r: &mut impl Read) -> Result<Context> {
    let key = keyr(r)?;
    let side = match u8r(r)? {
        0 => BookSide::Black,
        1 => BookSide::White,
        _ => return Err(invalid("generation side")),
    };
    Ok(Context { key, side })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_play_with_manual_and_legacy_searches_rebuilds_quality_after_validation() {
        let key = Key::new(Board::new()).0;
        let identity = [7; 32];
        let actions = legal_actions(key.board());
        let inputs: Vec<_> = actions
            .iter()
            .enumerate()
            .map(|(i, _)| {
                Assessment::import(ImportedValue {
                    score: if i == 0 { 4 } else { 0 },
                    quality: AnalysisQuality {
                        depth: if i == 0 { 0 } else { 12 },
                        selectivity: Selectivity::Full,
                        solved: false,
                    },
                    bound: Bound::Exact,
                    identity,
                    origin: if i == 0 {
                        ValueOrigin::Manual
                    } else {
                        ValueOrigin::Imported
                    },
                })
                .unwrap()
            })
            .collect();
        let ids: Vec<_> = inputs.iter().map(|v| v.content_id()).collect();
        let digest: [u8; 32] = Sha256::digest(ids.concat()).into();
        let old_value = Assessment::new(
            ValueClaim::Estimate {
                score: 4,
                bound: EstimateBound::Point,
            },
            ValueAnalysis::Derived {
                basis: Derivation::Maximum {
                    root: [key.0, key.1],
                    policy: 1,
                    inputs: digest,
                },
                summary: legacy_summary(inputs.iter().copied()),
            },
            identity,
        )
        .unwrap();
        let mut catalog = catalog::Catalog::default();
        catalog.derivations.insert(digest, ids);
        let mut pool = ValuePool::default();
        for &v in &inputs {
            pool.intern(v);
        }
        pool.intern(old_value);
        validate_catalog(&catalog, &pool.values).unwrap();
        let path = std::env::temp_dir().join(format!(
            "deft-old-mixed-{}-{}.book",
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let _cleanup = TempFile(path.clone());
        atomic_save(&path, BOOK, 3, |w| {
            w.write_all(&identity)?;
            u8w(w, 1)?;
            stringw(w, "old mixed quality fixture")?;
            u64w(w, 1)?;
            u64w(w, actions.len() as u64)?;
            u64w(w, pool.values.len() as u64)?;
            for v in &pool.values {
                value_codec::write(w, v)?;
            }
            u64w(w, 0)?; // profiles
            u64w(w, 0)?; // runs
            u64w(w, 1)?; // old derivation input lists
            w.write_all(&digest)?;
            u8w(w, inputs.len() as u8)?;
            for v in &inputs {
                w.write_all(&v.content_id())?;
            }
            keyw(w, key)?;
            u8w(w, actions.len() as u8)?;
            u8w(w, 1)?;
            refw(w, Some(pool.id(old_value)))?;
            refw(w, None)?;
            for (&action, &value) in actions.iter().zip(&inputs) {
                u8w(w, action_code(action))?;
                u8w(w, 0)?;
                refw(w, Some(pool.id(value)))?;
                refw(w, None)?;
            }
            extensionsw(
                w,
                &[Extension {
                    kind: 71,
                    bytes: b"old quality cache".to_vec(),
                }],
            )
        })
        .unwrap();
        let loaded = load_book(&path, ReadLimits::default()).unwrap();
        assert!(loaded.extensions.is_empty());
        let value = loaded.probe(key.board()).unwrap().value().unwrap();
        assert_eq!(value.score(), 4);
        assert_eq!(value.analysis().summary().legacy_depth, 12);
        loaded.save(&path).unwrap();
        assert_eq!(
            load_book(&path, ReadLimits::default())
                .unwrap()
                .probe(key.board())
                .unwrap()
                .value(),
            Some(value)
        );
    }
    fn signed_write(path: &Path, bytes: &mut [u8]) {
        let hash: [u8; 32] = Sha256::digest(&bytes[HEADER as usize..]).into();
        bytes[24..56].copy_from_slice(&hash);
        std::fs::write(path, bytes).unwrap();
    }
    fn legacy_configw(w: &mut dyn Write, c: &GenerationConfig) -> Result<()> {
        let mut modern = Vec::new();
        configw(&mut modern, c)?;
        u8w(w, u8::from(c.reuse_completed))?;
        let default = GenerationConfig::default();
        let stages = c
            .analysis
            .stages()
            .unwrap_or_else(|| default.analysis.stages().unwrap());
        u8w(w, stages.len() as u8)?;
        for v in stages {
            u8w(w, v.depth)?;
            u8w(w, v.selectivity as u8)?;
        }
        let levels = c.analysis.levels();
        u8w(w, levels.map_or(0, |v| v.len()) as u8)?;
        if let Some(v) = levels {
            for &n in v {
                u8w(w, n)?;
            }
        }
        let prefix = 3 + c.analysis.len() * if c.analysis.levels().is_some() { 1 } else { 2 };
        w.write_all(&modern[prefix..])?;
        Ok(())
    }
    #[test]
    fn generation_config_reads_v1_v2_and_bounds_v3_settings() {
        let original = GenerationConfig {
            legacy: LegacyPolicy::RecordedQuality,
            ..Default::default()
        };
        let mut bytes = Vec::new();
        legacy_configw(&mut bytes, &original).unwrap();
        let level_offset = 2 + 2 * original.analysis.stages().unwrap().len();
        // v1 has exactly the same fields except the level count and levels.
        let mut legacy = bytes[..bytes.len() - 13].to_vec();
        legacy.remove(level_offset);
        assert_eq!(configr(&mut legacy.as_slice(), 1).unwrap(), original);
        let v2 = &bytes[..bytes.len() - 13];
        assert_eq!(configr(&mut &v2[..], 2).unwrap(), original);
        assert_eq!(configr(&mut bytes.as_slice(), 3).unwrap(), original);
        bytes[level_offset] = 61;
        assert!(configr(&mut bytes.as_slice(), 2).is_err());
        let mut cfg = original;
        cfg.analysis = AnalysisSchedule::Levels(vec![2, 4]);
        cfg.cumulative_loss = Some(5);
        cfg.leaf_loss = Some(2);
        cfg.frontier_only = true;
        cfg.schedule = SchedulePolicy::Frontier;
        let mut bytes = Vec::new();
        legacy_configw(&mut bytes, &cfg).unwrap();
        assert_eq!(configr(&mut bytes.as_slice(), 5).unwrap(), cfg);
        let mut modern = cfg.clone();
        modern.line_plies = 12;
        modern.max_parallel_jobs = 3;
        let mut modern_bytes = Vec::new();
        legacy_configw(&mut modern_bytes, &modern).unwrap();
        assert_eq!(configr(&mut modern_bytes.as_slice(), 6).unwrap(), modern);
        let mut v4 = cfg.clone();
        v4.schedule = SchedulePolicy::Breadth;
        assert_eq!(configr(&mut &bytes[..bytes.len() - 1], 4).unwrap(), v4);
        let schedule_offset = bytes.len() - 7;
        bytes[schedule_offset] = 2;
        assert!(configr(&mut bytes.as_slice(), 5).is_err());
        bytes[level_offset + 2] = 1; // decreasing levels, valid byte encoding
        assert!(configr(&mut bytes.as_slice(), 2).is_err());
    }
    #[test]
    fn retarget_keeps_inactive_manual_source_through_work_round_trip() {
        use deft_reversi_engine::{EngineOptions, Model, PatternModel};
        let engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let old_id = [42; 32];
        let mut editor = BookEditor::new(old_id);
        let board = Board::new();
        let action = legal_actions(board)[0];
        let value = Assessment::import(ImportedValue {
            score: 3,
            quality: AnalysisQuality {
                depth: 12,
                selectivity: Selectivity::Full,
                solved: false,
            },
            bound: Bound::Exact,
            identity: old_id,
            origin: ValueOrigin::Manual,
        })
        .unwrap();
        editor.record_manual(board, action, value).unwrap();
        editor.retarget(&engine).unwrap();
        let dir = std::env::temp_dir().join(format!("deft-book-retarget-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("work");
        editor.save(&path).unwrap();
        let loaded = BookEditor::load(&path, ReadLimits::default()).unwrap();
        let (key, t) = Key::new(board);
        let edge = loaded.nodes[&key]
            .edges
            .iter()
            .find(|e| e.action == t.action(action))
            .unwrap();
        assert_eq!(loaded.values.opt(edge.manual()), Some(value));
        assert_eq!(loaded.effective(key, edge), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn work_policy_is_required_in_v4_and_legacy_work_defaults_to_quality() {
        let dir = std::env::temp_dir().join(format!("deft-book-policy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("work");
        let mut editor = BookEditor::new([7; 32]);
        editor.insert_position(Board::new()).unwrap();
        editor.save(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let policy_offset = HEADER as usize + 32;
        assert_eq!(original[policy_offset], 1);
        let mut invalid = original.clone();
        invalid[policy_offset] = 2;
        signed_write(&path, &mut invalid);
        assert!(BookEditor::load(&path, ReadLimits::default()).is_err());
        for version in 1u32..=3 {
            let mut legacy = include_bytes!("../tests/fixtures/empty-v6.work").to_vec();
            legacy.remove(legacy.len() - 10); // v6 absent frontier flag
            legacy.remove(policy_offset);
            legacy[8..12].copy_from_slice(&version.to_le_bytes());
            let size = legacy.len() as u64 - HEADER;
            legacy[16..24].copy_from_slice(&size.to_le_bytes());
            signed_write(&path, &mut legacy);
            let loaded = BookEditor::load(&path, ReadLimits::default()).unwrap();
            assert_eq!(loaded.propagation, PropagationPolicy::Book);
            assert_eq!(loaded.len(), 1);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn valid_checksum_does_not_bypass_semantic_validation() {
        let dir = std::env::temp_dir().join(format!("deft-book-malformed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("input");
        let mut editor = BookEditor::new([0; 32]);
        editor.insert_position(Board::new()).unwrap();
        editor.export().unwrap().save(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let counts = HEADER as usize + 32 + 1 + 4 + editor.producer.len();
        let mut cursor = std::io::Cursor::new(&original[HEADER as usize..]);
        idr(&mut cursor).unwrap();
        u8r(&mut cursor).unwrap();
        stringr(&mut cursor).unwrap();
        self::counts(&mut cursor, ReadLimits::default()).unwrap();
        poolr(&mut cursor, ReadLimits::default(), true).unwrap();
        catalogr(&mut cursor, ReadLimits::default(), true).unwrap();
        let key = HEADER as usize + cursor.position() as usize;
        let first_action = key + 16 + 1 + 1 + 8;
        let mut bytes = original.clone();
        bytes[first_action] = 64;
        signed_write(&path, &mut bytes);
        assert!(Book::load(&path, ReadLimits::default()).is_err());
        let mut bytes = original.clone();
        bytes[counts..counts + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        signed_write(&path, &mut bytes);
        assert!(Book::load(&path, ReadLimits::default()).is_err());
        let mut bytes = original.clone();
        let player = bytes[key..key + 8].to_vec();
        bytes[key + 8..key + 16].copy_from_slice(&player);
        signed_write(&path, &mut bytes);
        assert!(Book::load(&path, ReadLimits::default()).is_err());
        let mut bytes = original.clone();
        bytes[first_action + 1] = 1;
        signed_write(&path, &mut bytes);
        assert!(Book::load(&path, ReadLimits::default()).is_err());
        let mut bytes = original.clone();
        bytes[key + 17] = 1;
        signed_write(&path, &mut bytes);
        assert!(Book::load(&path, ReadLimits::default()).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn unknown_optional_sections_round_trip_and_required_sections_are_rejected() {
        let dir = std::env::temp_dir().join(format!("deft-book-extensions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("work");
        let mut e = BookEditor::new([0; 32]);
        e.insert_position(Board::new()).unwrap();
        e.extensions.push(Extension {
            kind: 77,
            bytes: vec![1, 2, 3],
        });
        e.save(&path).unwrap();
        let loaded = BookEditor::load(&path, ReadLimits::default()).unwrap();
        assert_eq!(loaded.extensions[0].kind, 77);
        assert_eq!(loaded.extensions[0].bytes, vec![1, 2, 3]);
        let play = dir.join("play");
        loaded.export().unwrap().save(&play).unwrap();
        let book = Book::load(&play, ReadLimits::default()).unwrap();
        assert_eq!(book.extensions[0].bytes, vec![1, 2, 3]);
        let copy = dir.join("copy");
        book.save(&copy).unwrap();
        assert_eq!(std::fs::read(&play).unwrap(), std::fs::read(&copy).unwrap());
        let mut bytes = std::fs::read(&play).unwrap();
        let kind = bytes.len() - 15;
        bytes[kind + 3] |= 0x80;
        signed_write(&play, &mut bytes);
        assert!(Book::load(&play, ReadLimits::default()).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn subset_and_candidate_evidence_cannot_be_reinterpreted_as_a_whole_child() {
        use deft_reversi_engine::{EngineOptions, Goal, Model, PatternModel, SearchRequest};
        let mut engine = Engine::new(
            Model::from_pattern(PatternModel::zero()),
            EngineOptions::default().tt_mib(1),
        )
        .unwrap();
        let parent = Board::new();
        let child = parent
            .play(Action::Place(parent.legal_moves().next().unwrap()))
            .unwrap()
            .canonical();
        let all = child
            .legal_moves()
            .fold(0, |mask, s| mask | (1u64 << s.index()));
        let one = 1u64 << child.legal_moves().next().unwrap().index();
        assert_ne!(one, all);
        let mut make = |mask| {
            let result = engine
                .search(child, &SearchRequest::new(Goal::Depth(2)).root_moves(mask))
                .unwrap();
            Assessment::from_search_result(
                child,
                RequestedAnalysis::External,
                SearchKind::Midgame {
                    depth: 2,
                    selectivity: Selectivity::Full,
                },
                &result,
                analysis_identity(&engine).unwrap(),
            )
            .unwrap()
        };
        let subset = make(one);
        let whole = make(all);
        let candidate = whole.project(SearchProjection::BestMove).unwrap();
        let catalog = catalog::Catalog::default();
        for input in [subset, candidate] {
            assert!(input.try_child_to_parent(true).is_err());
        }
        assert!(validate_catalog(&catalog, &[whole, whole.child_to_parent(true)]).is_ok());
    }
}

#[cfg(test)]
mod migration_queue_tests {
    use super::*;
    #[test]
    fn old_quality_work_rebuilds_jobs_without_losing_completed_search_records() {
        let bytes = include_bytes!("../tests/fixtures/search-v7.work");
        assert_eq!(bytes[HEADER as usize + 32], 0); // old quality-first mode
        let path = std::env::temp_dir().join(format!("deft-legacy-requeue-{}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let editor = BookEditor::load(&path, ReadLimits::default()).unwrap();
        assert!(editor.migration_report().unwrap().jobs_rebuilt);
        let g = editor.generation.as_ref().unwrap();
        assert!(g.expanded.is_empty());
        assert!(g.deepen.is_empty());
        for (&c, &distance) in &g.distances {
            if !c.key.board().is_terminal() {
                assert!(g.fresh.contains(&(distance, c)));
            }
        }
        assert!(editor.nodes.values().all(|n| n.completed_stages == 0));
        assert!(
            editor
                .values
                .iter()
                .any(|v| matches!(v.analysis(), ValueAnalysis::Search { .. }))
        );
        std::fs::remove_file(path).unwrap();
    }
}
