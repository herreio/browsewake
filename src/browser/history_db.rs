use crate::model::{VisitEntry, Window};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Chrome timestamps are microseconds since 1601-01-01.
/// Subtract this to get Unix epoch seconds.
const CHROME_EPOCH_OFFSET_US: i64 = 11_644_473_600_000_000;

fn chrome_time_to_unix_secs(chrome_us: i64) -> i64 {
    (chrome_us - CHROME_EPOCH_OFFSET_US) / 1_000_000
}

/// Augment tabs in the given windows with deep history from the Chromium History database.
/// Anchors to SNSS URLs so only causally connected visits are included.
pub fn augment_windows(profile_dir: &Path, windows: &mut [Window], browser_name: &str) {
    let history_path = profile_dir.join("History");
    if !history_path.exists() {
        return;
    }

    // Collect (tab_id, anchor_urls) pairs from SNSS data.
    let mut tab_anchors: Vec<(i32, Vec<String>)> = Vec::new();
    for window in windows.iter() {
        for tab in &window.tabs {
            if let Some(id) = tab.tab_id {
                let mut urls: Vec<String> = tab.history.iter().map(|e| e.url.clone()).collect();
                if urls.is_empty() {
                    urls.push(tab.url.clone());
                }
                tab_anchors.push((id, urls));
            }
        }
    }

    if tab_anchors.is_empty() {
        return;
    }

    let history_map = match read_anchored_history(&history_path, &tab_anchors) {
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

/// Copy the History file and its companion files to a temp location.
fn open_history_db_copy(
    history_path: &Path,
) -> Result<(Connection, PathBuf), rusqlite::Error> {
    let tmp = std::env::temp_dir().join(format!("browsewake-history-{}", std::process::id()));
    let copy_err = |e: std::io::Error| {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
            Some(format!("failed to copy History DB: {e}")),
        )
    };
    std::fs::copy(history_path, &tmp).map_err(copy_err)?;

    // Copy companion files so SQLite can recover uncommitted state.
    // Journal mode: History-journal. WAL mode: History-wal, History-shm.
    let stem = history_path.file_name().unwrap().to_string_lossy();
    let tmp_stem = tmp.file_name().unwrap().to_string_lossy().to_string();
    for suffix in ["-journal", "-wal", "-shm"] {
        let companion = history_path.with_file_name(format!("{stem}{suffix}"));
        if companion.exists() {
            let tmp_companion = tmp.with_file_name(format!("{tmp_stem}{suffix}"));
            let _ = std::fs::copy(&companion, &tmp_companion);
        }
    }

    let conn = Connection::open_with_flags(&tmp, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok((conn, tmp))
}

fn cleanup_temp(tmp: &Path) {
    let _ = std::fs::remove_file(tmp);
    let stem = tmp.file_name().unwrap().to_string_lossy().to_string();
    for suffix in ["-journal", "-wal", "-shm"] {
        let _ = std::fs::remove_file(tmp.with_file_name(format!("{stem}{suffix}")));
    }
}

/// Read history anchored to SNSS URLs, walking from_visit backward to recover
/// only the causally connected chain for each tab.
fn read_anchored_history(
    history_path: &Path,
    tab_anchors: &[(i32, Vec<String>)],
) -> Result<HashMap<i32, Vec<VisitEntry>>, rusqlite::Error> {
    let (conn, tmp_path) = open_history_db(history_path)?;

    let mut map: HashMap<i32, Vec<VisitEntry>> = HashMap::new();

    for (tab_id, anchor_urls) in tab_anchors {
        if let Ok(visits) = walk_from_visit_chain(&conn, *tab_id, anchor_urls)
            && !visits.is_empty()
        {
            map.insert(*tab_id, visits);
        }
    }

    drop(conn);
    if let Some(ref tmp) = tmp_path {
        cleanup_temp(tmp);
    }

    Ok(map)
}

/// Find the earliest visit matching an anchor URL for this tab_id,
/// then walk from_visit backward to build the full causal chain,
/// and forward to include the complete connected subgraph.
fn walk_from_visit_chain(
    conn: &Connection,
    tab_id: i32,
    anchor_urls: &[String],
) -> Result<Vec<VisitEntry>, rusqlite::Error> {
    if anchor_urls.is_empty() {
        return Ok(Vec::new());
    }

    // Find all visit IDs for this tab_id that match any anchor URL.
    // Pick the earliest one as the root anchor.
    let url_placeholders: String = anchor_urls.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let anchor_sql = format!(
        "SELECT v.id
        FROM visits v
        JOIN urls u ON u.id = v.url
        JOIN context_annotations ca ON ca.visit_id = v.id
        WHERE ca.tab_id = ? AND u.url IN ({url_placeholders})
        ORDER BY v.visit_time ASC
        LIMIT 1"
    );

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(tab_id));
    for url in anchor_urls {
        params.push(Box::new(url.clone()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params.iter().map(|p| p.as_ref()).collect();

    let earliest_anchor: Option<i64> =
        conn.query_row(&anchor_sql, param_refs.as_slice(), |row| row.get(0)).ok();

    let anchor_id = match earliest_anchor {
        Some(id) => id,
        None => return Ok(Vec::new()),
    };

    // Walk backward from the anchor via from_visit to find the root,
    // then walk forward collecting only visits that belong to this tab_id
    // (or lack annotations, e.g. redirect intermediaries).
    let chain_sql =
        "WITH RECURSIVE
            -- Walk backward from anchor to find the chain root
            backward(vid) AS (
                SELECT ?1
                UNION ALL
                SELECT v.from_visit
                FROM backward b
                JOIN visits v ON v.id = b.vid
                WHERE v.from_visit != 0
            ),
            -- The root is the visit whose from_visit is 0 or not in our chain
            chain_root(vid) AS (
                SELECT v.id FROM backward b
                JOIN visits v ON v.id = b.vid
                WHERE v.from_visit = 0
                   OR v.from_visit NOT IN (SELECT vid FROM backward)
                ORDER BY v.visit_time ASC
                LIMIT 1
            ),
            -- Walk forward, only following descendants that belong to this
            -- tab_id or have no context_annotations (redirect intermediaries)
            forward(vid) AS (
                SELECT vid FROM chain_root
                UNION ALL
                SELECT v.id
                FROM forward f
                JOIN visits v ON v.from_visit = f.vid
                WHERE NOT EXISTS (
                    SELECT 1 FROM context_annotations ca
                    WHERE ca.visit_id = v.id AND ca.tab_id != ?2
                )
            )
        SELECT
            u.url,
            u.title,
            v.visit_time,
            fv_url.url AS from_url
        FROM forward f
        JOIN visits v ON v.id = f.vid
        JOIN urls u ON u.id = v.url
        LEFT JOIN visits fv ON fv.id = v.from_visit AND v.from_visit != 0
        LEFT JOIN urls fv_url ON fv_url.id = fv.url
        ORDER BY v.visit_time ASC";

    let mut stmt = conn.prepare(chain_sql)?;
    let mut rows = stmt.query(rusqlite::params![anchor_id, tab_id])?;

    let mut visits = Vec::new();
    while let Some(row) = rows.next()? {
        let url: String = row.get(0)?;
        let title: String = row.get(1)?;
        let visit_time: i64 = row.get(2)?;
        let from_url: Option<String> = row.get(3)?;

        visits.push(VisitEntry {
            url,
            title,
            visit_time: chrome_time_to_unix_secs(visit_time),
            from_url,
        });
    }

    Ok(visits)
}
