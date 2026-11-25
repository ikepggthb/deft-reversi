//! 書き込み途中のクラッシュによるファイル破損を防ぎ、直前の正常な状態をバックアップするモジュール。
//!
//! ファイルを直接上書きすると、書き込みの途中で電源が落ちたりアプリが異常終了したりした際に
//! 中途半端な内容でファイルが壊れる恐れがある。このモジュールでは「一時ファイルへ完全に書き出して
//! ディスクへ同期した後、名前変更で一気に差し替える（アトミック保存）」ことで破損を防ぐ。
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

/// ファイル名の末尾に接尾辞（`.tmp`, `.bak`, `.corrupt` 等）を付与したパスを返す。
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.file_name().unwrap_or_default().into();
    name.push(suffix);
    path.with_file_name(name)
}

/// 指定ファイルに対応するバックアップファイル（`.bak`）のパスを返す。
pub fn backup_path(path: &Path) -> PathBuf {
    with_suffix(path, ".bak")
}

/// ファイルを壊さないよう安全に上書き保存する。
///
/// 1. 同じフォルダに一時ファイル（`.tmp`）を作り、内容を書き込んで `sync_all` でディスクへ確実に反映させる
/// 2. 既存の本体ファイルがあれば `.bak` へコピーしてバックアップを残す
/// 3. 一時ファイルを本体ファイル名へリネームして差し替える
///
/// リネームは OS レベルで原子的に行われるため、本体ファイルが書きかけの状態で残ることはない。
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
            // リネームではなくコピーで退避することで、本体ファイルが一瞬でも欠損するのを防ぐ。
            fs::copy(path, backup_path(path))?;
        }
        rename_with_retry(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Windows 環境でウイルス対策ソフト等が一時的にファイルをロックしている場合に備え、
/// `PermissionDenied` 時に指数バックオフで最大 4 回リネームを再試行する。
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

/// 最大 `limit` バイトまでファイルを UTF-8 文字列として読み込む。上限超過時はエラーとする。
pub fn read(path: &Path, limit: u64) -> io::Result<String> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the file is too large",
        ));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// ファイル内容のパース結果。
///
/// `Unsupported` は新しいバージョンのアプリが作成した有効なファイルを示し、
/// 古い `.bak` へのフォールバックや `.corrupt` へのリネームを行わずに保護する。
pub enum ParseOutcome<T, E> {
    /// パース成功。
    Ok(T),
    /// ファイル内容が破損・不正。
    Corrupt(E),
    /// 未対応の新しいスキーマバージョン。
    Unsupported(E),
}

impl<T, E> From<Result<T, E>> for ParseOutcome<T, E> {
    fn from(result: Result<T, E>) -> Self {
        match result {
            Ok(value) => Self::Ok(value),
            Err(error) => Self::Corrupt(error),
        }
    }
}

/// 本体ファイル `path` を読み込み、失敗時は `.bak` へのフォールバックを試みる。
///
/// 双方とも失敗し、かつ本体ファイルが実際に破損していた場合（一時的な I/O エラーや
/// 未対応の新バージョンではない場合）は、上書きを免れるよう `.corrupt` にリネームしてから
/// 最初のエラーを返す。
pub fn read_with_backup<T, E>(
    path: &Path,
    limit: u64,
    parse: impl Fn(&str) -> ParseOutcome<T, E>,
) -> Result<T, String>
where
    E: std::fmt::Display,
{
    enum Failure {
        Io(String),
        Corrupt(String),
        Unsupported(String),
    }
    let attempt = |path: &Path| -> Result<T, Failure> {
        let text = read(path, limit).map_err(|e| {
            if e.kind() == io::ErrorKind::InvalidData {
                Failure::Corrupt(e.to_string())
            } else {
                Failure::Io(e.to_string())
            }
        })?;
        match parse(&text) {
            ParseOutcome::Ok(value) => Ok(value),
            ParseOutcome::Corrupt(e) => Err(Failure::Corrupt(e.to_string())),
            ParseOutcome::Unsupported(e) => Err(Failure::Unsupported(e.to_string())),
        }
    };
    match attempt(path) {
        Ok(value) => Ok(value),
        Err(Failure::Unsupported(error)) => Err(error),
        Err(first) => {
            let (error, corrupt) = match first {
                Failure::Corrupt(error) => (error, true),
                Failure::Io(error) => (error, false),
                Failure::Unsupported(_) => unreachable!(),
            };
            match attempt(&backup_path(path)) {
                Ok(value) => Ok(value),
                Err(_) => {
                    if corrupt && path.exists() {
                        let _ = fs::rename(path, with_suffix(path, ".corrupt"));
                    }
                    Err(error)
                }
            }
        }
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

        let parse = |s: &str| ParseOutcome::from(s.parse::<u8>().map_err(|e| e.to_string()));
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

    #[test]
    fn unsupported_version_is_left_untouched_and_does_not_fall_back() {
        let dir = temp_dir("atomic-unsupported");
        let path = dir.join("game.dreversi");
        write(&path, b"1").unwrap();
        write(&path, b"v2").unwrap();
        let parse = |s: &str| match s {
            "v2" => ParseOutcome::Unsupported("newer version"),
            other => ParseOutcome::from(other.parse::<u8>().map_err(|_| "corrupt")),
        };
        // Neither falls back to the older backup nor renames to .corrupt.
        assert!(read_with_backup(&path, 10, parse).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "v2");
        assert!(!with_suffix(&path, ".corrupt").exists());
        // Same without a backup file.
        let _ = fs::remove_file(backup_path(&path));
        assert!(read_with_backup(&path, 10, parse).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "v2");
        assert!(!with_suffix(&path, ".corrupt").exists());
        let _ = fs::remove_dir_all(dir);
    }
}
