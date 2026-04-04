use crate::model::{VisitEntry, Window};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Chrome timestamps are microseconds since 1601-01-01.
/// Subtract this to get Unix epoch microseconds.
const CHROME_EPOCH_OFFSET_US: i64 = 11_644_473_600_000_000;

fn chrome_time_to_unix_secs(chrome_us: i64) -> i64 {
    (chrome_us - CHROME_EPOCH_OFFSET_US) / 1_000_000
}

/// Augment tabs in the given windows with deep history from the Chromium History database.
pub fn augment_windows(profile_dir: &Path, windows: &mut [Window], browser_name: &str) {
    let history_path = profile_dir.join("History");
    if !history_path.exists() {
        return;
    }

    let tab_ids: Vec<i32> = windows
        .iter()
        .flat_map(|w| w.tabs.iter())
        .filter_map(|t| t.tab_id)
        .collect();

    if tab_ids.is_empty() {
        return;
    }

    let history_map = match read_history_for_tabs(&history_path, &tab_ids) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("warning: could not read {} History DB: {e}", browser_name);
            return;
        }
    };

    for window in windows.iter_mut() {
        for tab in window.tabs.iter_mut() {
            if let Some(id) = tab.tab_id
                && let Some(visits) = history_map.get(&id)
            {
                tab.deep_history = visits.clone();
            }
        }
    }
}

/// Open the History database, copying to a temp file if the original is locked by the browser.
fn open_history_db(history_path: &Path) -> Result<(Connection, Option<PathBuf>), rusqlite::Error> {
    let conn = Connection::open_with_flags(history_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    match conn.execute_batch("SELECT 1 FROM urls LIMIT 0") {
        Ok(()) => Ok((conn, None)),
        Err(_) => {
            drop(conn);
            let (conn, tmp) = open_history_db_copy(history_path)?;
            Ok((conn, Some(tmp)))
        }
    }
}

fn open_history_db_copy(
    history_path: &Path,
) -> Result<(Connection, PathBuf), rusqlite::Error> {
    let tmp = std::env::temp_dir().join(format!("browsewake-history-{}", std::process::id()));
    std::fs::copy(history_path, &tmp).map_err(|e| {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
            Some(format!("failed to copy History DB: {e}")),
        )
    })?;
    let conn = Connection::open_with_flags(&tmp, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok((conn, tmp))
}

fn read_history_for_tabs(
    history_path: &Path,
    tab_ids: &[i32],
) -> Result<HashMap<i32, Vec<VisitEntry>>, rusqlite::Error> {
    let (conn, tmp_path) = open_history_db(history_path)?;

    let result = query_history(&conn, tab_ids);

    drop(conn);
    if let Some(tmp) = tmp_path {
        let _ = std::fs::remove_file(&tmp);
    }

    result
}

fn query_history(
    conn: &Connection,
    tab_ids: &[i32],
) -> Result<HashMap<i32, Vec<VisitEntry>>, rusqlite::Error> {
    let placeholders: String = tab_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT
            ca.tab_id,
            u.url,
            u.title,
            v.visit_time,
            fv_url.url AS from_url
        FROM context_annotations ca
        JOIN visits v ON v.id = ca.visit_id
        JOIN urls u ON u.id = v.url
        LEFT JOIN visits fv ON fv.id = v.from_visit AND v.from_visit != 0
        LEFT JOIN urls fv_url ON fv_url.id = fv.url
        WHERE ca.tab_id IN ({placeholders})
        ORDER BY ca.tab_id, v.visit_time ASC"
    );

    let params: Vec<Box<dyn rusqlite::types::ToSql>> = tab_ids
        .iter()
        .map(|id| Box::new(*id) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(param_refs.as_slice())?;

    let mut map: HashMap<i32, Vec<VisitEntry>> = HashMap::new();

    while let Some(row) = rows.next()? {
        let tab_id: i32 = row.get(0)?;
        let url: String = row.get(1)?;
        let title: String = row.get(2)?;
        let visit_time: i64 = row.get(3)?;
        let from_url: Option<String> = row.get(4)?;

        map.entry(tab_id).or_default().push(VisitEntry {
            url,
            title,
            visit_time: chrome_time_to_unix_secs(visit_time),
            from_url,
        });
    }

    Ok(map)
}
