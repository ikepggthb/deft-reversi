//! Writes that leave either the old file or the new one, never a torn file,
//! and a backup of the last good version.
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.file_name().unwrap_or_default().into();
    name.push(suffix);
    path.with_file_name(name)
}

pub fn backup_path(path: &Path) -> PathBuf {
    with_suffix(path, ".bak")
}

/// Writes a temporary file in the same folder, syncs it, keeps the current
/// file as `.bak`, and renames the new one over it.
pub fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temporary = with_suffix(path, ".tmp");
    let result = (|| {
        let mut file = File::create(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        if path.exists() {
            // A copy, so the file itself is never missing.
            fs::copy(path, backup_path(path))?;
        }
        rename_with_retry(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// On Windows a virus scanner may hold the target for a moment.
fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) if attempt < 4 && error.kind() == io::ErrorKind::PermissionDenied => {
                attempt += 1;
                thread::sleep(Duration::from_millis(25 << attempt));
            }
            Err(error) => return Err(error),
        }
    }
}

/// Reads at most `limit` bytes; a larger file is an error.
pub fn read(path: &Path, limit: u64) -> io::Result<String> {
    if fs::metadata(path)?.len() > limit {
        return Err(io::Error::other("the file is too large"));
    }
    fs::read_to_string(path)
}

/// Reads and parses `path`, then its backup. When both fail, the broken
/// file is kept as `.corrupt` so it is not overwritten, and the first error
/// is returned.
pub fn read_with_backup<T, E>(
    path: &Path,
    limit: u64,
    parse: impl Fn(&str) -> Result<T, E>,
) -> Result<T, String>
where
    E: std::fmt::Display,
{
    let attempt = |path: &Path| -> Result<T, String> {
        let text = read(path, limit).map_err(|e| e.to_string())?;
        parse(&text).map_err(|e| e.to_string())
    };
    match attempt(path) {
        Ok(value) => Ok(value),
        Err(error) => match attempt(&backup_path(path)) {
            Ok(value) => Ok(value),
            Err(_) => {
                if path.exists() {
                    let _ = fs::rename(path, with_suffix(path, ".corrupt"));
                }
                Err(error)
            }
        },
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("deft-reversi-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_keep_a_backup_and_recover_from_corruption() {
        let dir = temp_dir("atomic");
        let path = dir.join("a.json");
        write(&path, b"1").unwrap();
        write(&path, b"2").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "2");
        assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), "1");
        assert!(!with_suffix(&path, ".tmp").exists());

        let parse = |s: &str| s.parse::<u8>().map_err(|e| e.to_string());
        // A torn write of the current file falls back to the backup.
        fs::write(&path, "x").unwrap();
        assert_eq!(read_with_backup(&path, 10, parse), Ok(1));
        // Both broken: the file is set aside and nothing is returned.
        fs::write(backup_path(&path), "y").unwrap();
        assert!(read_with_backup(&path, 10, parse).is_err());
        assert!(!path.exists());
        assert!(with_suffix(&path, ".corrupt").exists());
        // Too large is an error, not a long read.
        fs::write(&path, "12345").unwrap();
        assert!(read(&path, 4).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
