//! 対局の識別情報（自動保存ファイル用の ID と開始日時）を扱うモジュール。
use std::{
    hash::{BuildHasher, RandomState},
    time::{SystemTime, UNIX_EPOCH},
};

/// 1 局ごとの一意な識別情報。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameInfo {
    /// 16 進数文字列のみで構成された ID（ファイル名として安全に使用可能）。
    pub id: String,
    /// 対局開始日時（UTC の RFC 3339 形式）。
    pub created_at: String,
}

impl GameInfo {
    /// 現在時刻とランダムソルトから新しい [`GameInfo`] を生成する。
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let millis = now.as_millis() as u64;
        // 同一ミリ秒内に開始された対局同士でも ID が衝突しないようソルトを付与する。
        let salt = RandomState::new().hash_one(millis) as u16;
        Self {
            id: format!("{millis:012x}{salt:04x}"),
            created_at: rfc3339(now.as_secs()),
        }
    }

    /// `id` が [`GameInfo::new`] で生成され得る安全な形式かを検証する。
    ///
    /// ファイルから読み込んだ ID をパスへ組み込む前のパストラバーサル対策として使用する。
    pub fn valid_id(id: &str) -> bool {
        (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

/// 現在時刻を UTC の RFC 3339 文字列（`YYYY-MM-DDThh:mm:ssZ`）として返す。
pub fn now_rfc3339() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339(seconds)
}

/// UNIX エポックからの経過秒数を UTC の `YYYY-MM-DDThh:mm:ssZ` 形式へ変換する。
fn rfc3339(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let time = seconds % 86_400;
    // Howard Hinnant の days-to-civil アルゴリズムで西暦年月日を求める。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3_600,
        time / 60 % 60,
        time % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_ids() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(1_793_697_905), "2026-11-03T09:25:05Z");
        let info = GameInfo::new();
        assert!(GameInfo::valid_id(&info.id));
        assert_ne!(info.id, GameInfo::new().id);
        assert!(!GameInfo::valid_id("../x"));
    }
}
