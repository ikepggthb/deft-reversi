//! Versioned, bounded streaming codec. No Rust layout or unbounded serde vectors.
use super::*;
use super::{
    editor::{Edge, Node, Pending},
    generate::{Context, GenerationState},
    snapshot::{CompactValue, Record, StoredCandidate},
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const BOOK: &[u8; 8] = b"DFTBK001";
const WORK: &[u8; 8] = b"DFTBW001";
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
fn atomic_save(
    path: &Path,
    magic: &[u8; 8],
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
    u32w(&mut file, 1)?;
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
fn open(path: &Path, magic: &[u8; 8], limits: ReadLimits) -> Result<BufReader<File>> {
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
    if u32r(&mut file)? != 1 || u32r(&mut file)? != 1 {
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
    Ok(file)
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
fn valuew(w: &mut dyn Write, value: Option<Assessment>, full: bool) -> Result<()> {
    u8w(w, u8::from(value.is_some()))?;
    if let Some(v) = value {
        v.validate()?;
        w.write_all(&v.score.to_le_bytes())?;
        u8w(w, v.quality.depth)?;
        u8w(w, v.quality.selectivity as u8)?;
        u8w(w, u8::from(v.quality.solved))?;
        u8w(
            w,
            match v.bound {
                Bound::Exact => 0,
                Bound::Lower => 1,
                Bound::Upper => 2,
            },
        )?;
        u8w(
            w,
            match v.origin {
                ValueOrigin::Search => 0,
                ValueOrigin::Manual => 1,
                ValueOrigin::Imported => 2,
                ValueOrigin::Rules => 3,
            },
        )?;
        if full {
            w.write_all(&v.identity)?;
        }
    }
    Ok(())
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
        _ => return Err(invalid("bound")),
    };
    let origin = match u8r(r)? {
        0 => ValueOrigin::Search,
        1 => ValueOrigin::Manual,
        2 => ValueOrigin::Imported,
        3 => ValueOrigin::Rules,
        _ => return Err(invalid("origin")),
    };
    let v = Assessment {
        score: i16::from_le_bytes(b),
        quality: AnalysisQuality {
            depth,
            selectivity: sel,
            solved,
        },
        bound,
        origin,
        identity: if full { idr(r)? } else { id },
    };
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
    atomic_save(path, BOOK, |w| {
        w.write_all(&book.identity)?;
        stringw(w, &book.producer)?;
        u64w(w, book.records.len() as u64)?;
        u64w(w, book.candidates.len() as u64)?;
        for record in &book.records {
            keyw(w, record.key)?;
            u8w(w, record.len as u8)?;
            u8w(w, u8::from(record.complete))?;
            valuew(w, record.value.map(|v| v.expand(book.identity)), false)?;
            for c in &book.candidates[record.start..record.start + record.len] {
                u8w(w, action_code(c.action))?;
                u8w(w, u8::from(c.registered))?;
                valuew(w, c.value.map(|v| v.expand(book.identity)), false)?;
            }
        }
        extensionsw(w, &book.extensions)
    })
}
pub(super) fn load_book(path: &Path, limits: ReadLimits) -> Result<Book> {
    let mut input = open(path, BOOK, limits)?;
    let identity = idr(&mut input)?;
    let producer = stringr(&mut input)?;
    let (n, total) = counts(&mut input, limits)?;
    let resident = (n as u64).saturating_mul(128).saturating_add(
        (total as u64).saturating_mul(std::mem::size_of::<StoredCandidate>() as u64),
    );
    if resident > limits.max_bytes {
        return Err(BookError::Limit("resident allocation"));
    }
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
        let value = valuer(&mut input, identity, false)?;
        let start = candidates.len();
        for legal in legal {
            let action = action_from(u8r(&mut input)?)?;
            if action != legal {
                return Err(invalid("illegal or duplicate candidate"));
            }
            let registered = boolr(&mut input)?;
            let value = valuer(&mut input, identity, false)?;
            if let Some(v) = value {
                v.validate_at(key.board())?;
            }
            candidates.push(StoredCandidate {
                action,
                registered,
                value: value.map(CompactValue::new),
            });
        }
        records.push(Record {
            key,
            start,
            len,
            complete,
            value: value.map(CompactValue::new),
        });
    }
    if candidates.len() != total {
        return Err(invalid("candidate total"));
    }
    let extensions = extensionsr(&mut input, limits.max_bytes.saturating_sub(resident))?;
    done(&mut input)?;
    let book = Book::from_parts(identity, producer, extensions, records, candidates)?;
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
        let all = entries
            .iter()
            .all(|c| c.value.is_some_and(|v| v.bound == Bound::Exact));
        if rec.complete != all {
            return Err(invalid("coverage summary"));
        }
        let expected = if entries.is_empty() {
            Some(terminal_value(rec.key.board()))
        } else if all {
            let mut v = entries
                .iter()
                .filter_map(|c| c.value)
                .reduce(|a, b| if b.score > a.score { b } else { a })
                .unwrap()
                .expand(identity);
            v.quality.depth = entries
                .iter()
                .filter_map(|c| c.value)
                .map(|v| v.depth)
                .min()
                .unwrap();
            v.quality.selectivity = selectivity(
                entries
                    .iter()
                    .filter_map(|c| c.value)
                    .map(|v| v.selectivity)
                    .min()
                    .unwrap(),
            )?;
            v.quality.solved = entries.iter().all(|c| c.value.unwrap().solved);
            if v.quality.solved {
                v.origin = ValueOrigin::Search;
            }
            Some(v)
        } else {
            None
        };
        let actual = rec.value.map(|v| v.expand(identity));
        if expected.map(|mut v| {
            v.identity = identity;
            v
        }) != actual
        {
            return Err(invalid("position value summary"));
        }
    }
    Ok(book)
}
pub(super) fn save_editor(editor: &BookEditor, path: &Path) -> Result<()> {
    editor.validate()?;
    atomic_save(path, WORK, |w| {
        w.write_all(&editor.identity)?;
        stringw(w, &editor.producer)?;
        u64w(w, editor.revision)?;
        u64w(w, editor.nodes.len() as u64)?;
        u64w(w, editor.nodes.values().map(|n| n.edges.len() as u64).sum())?;
        for (&key, n) in &editor.nodes {
            keyw(w, key)?;
            u64w(w, n.revision)?;
            u8w(w, n.edges.len() as u8)?;
            u64w(w, n.completed_stages as u64)?;
            for e in &n.edges {
                u8w(w, action_code(e.action))?;
                u8w(w, u8::from(e.registered))?;
                valuew(w, e.direct, true)?;
                valuew(w, e.manual, true)?;
            }
            u8w(w, u8::from(n.pending.is_some()))?;
            if let Some(p) = &n.pending {
                u64w(w, p.stage as u64)?;
                u64w(w, p.revision)?;
                for &v in &p.values {
                    valuew(w, v, true)?;
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
pub(super) fn load_editor(path: &Path, limits: ReadLimits) -> Result<BookEditor> {
    let mut input = open(path, WORK, limits)?;
    let identity = idr(&mut input)?;
    let producer = stringr(&mut input)?;
    let revision = u64r(&mut input)?;
    let (n, total) = counts(&mut input, limits)?;
    if (n as u64)
        .saturating_mul(512)
        .saturating_add((total as u64).saturating_mul(256))
        > limits.max_bytes
    {
        return Err(BookError::Limit("workspace allocation"));
    }
    let mut editor = BookEditor::new(identity);
    editor.producer = producer;
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
            let direct = valuer(&mut input, identity, true)?;
            let manual = valuer(&mut input, identity, true)?;
            edges.push(Edge {
                action,
                registered,
                direct,
                manual,
            });
        }
        let pending = if boolr(&mut input)? {
            let stage = limited_count(&mut input, 59)?;
            let revision = u64r(&mut input)?;
            let mut values = Vec::new();
            reserve(&mut values, len)?;
            for _ in 0..len {
                values.push(valuer(&mut input, identity, true)?);
            }
            Some(Pending {
                stage,
                revision,
                values,
            })
        } else {
            None
        };
        editor.ensure(key)?;
        let n = editor.nodes.get_mut(&key).unwrap();
        *n = Node {
            revision: node_revision,
            edges,
            value: None,
            complete: false,
            completed_stages: stages,
            pending,
        };
    }
    if count != total {
        return Err(invalid("candidate total"));
    }
    editor.revision = revision;
    if boolr(&mut input)? {
        let config = configr(&mut input)?;
        let iteration = u64r(&mut input)?;
        let mut queue_budget = limits.max_bytes.saturating_sub(
            (n as u64)
                .saturating_mul(512)
                .saturating_add((total as u64).saturating_mul(256)),
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
            let s = limited_count(&mut input, config.stages.len().saturating_sub(1))?;
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
            if n.completed_stages > config.stages.len()
                || n.pending.as_ref().is_some_and(|p| {
                    p.stage != n.completed_stages || p.stage >= config.stages.len()
                })
            {
                return Err(invalid("stage index"));
            }
        }
        editor.generation = Some(GenerationState {
            config,
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
    editor.extensions = extensionsr(
        &mut input,
        limits.max_bytes.saturating_sub(
            (n as u64)
                .saturating_mul(512)
                .saturating_add((total as u64).saturating_mul(256))
                .saturating_add(
                    editor
                        .generation
                        .as_ref()
                        .map_or(0, |g| g.estimated_bytes()),
                ),
        ),
    )?;
    done(&mut input)?;
    editor.repair()?;
    if editor.estimated_bytes() > limits.max_bytes {
        return Err(BookError::Limit("workspace memory"));
    }
    Ok(editor)
}
fn configw(w: &mut dyn Write, c: &GenerationConfig) -> Result<()> {
    u8w(w, u8::from(c.reuse_completed))?;
    u8w(w, c.stages.len() as u8)?;
    for s in &c.stages {
        u8w(w, s.depth)?;
        u8w(w, s.selectivity as u8)?;
    }
    u8w(w, c.new_job_weight)?;
    u8w(w, c.deepen_job_weight)?;
    u8w(w, c.black_loss.unwrap_or(255))?;
    u8w(w, c.white_loss.unwrap_or(255))?;
    u8w(w, c.expansion_loss)?;
    u8w(w, c.max_plies)?;
    u64w(w, c.max_memory_bytes)?;
    u64w(w, c.checkpoint_interval.as_secs())?;
    u32w(w, c.checkpoint_interval.subsec_nanos())
}
fn configr(r: &mut impl Read) -> Result<GenerationConfig> {
    let reuse_completed = boolr(r)?;
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
    let c = GenerationConfig {
        reuse_completed,
        stages,
        expansion_loss,
        black_loss,
        white_loss,
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
    fn signed_write(path: &Path, bytes: &mut [u8]) {
        let hash: [u8; 32] = Sha256::digest(&bytes[HEADER as usize..]).into();
        bytes[24..56].copy_from_slice(&hash);
        std::fs::write(path, bytes).unwrap();
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
        let counts = HEADER as usize + 32 + 4 + editor.producer.len();
        let key = counts + 16;
        let first_action = key + 16 + 1 + 1 + 1;
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
}
