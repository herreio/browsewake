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
fn open_history_db_copy(history_path: &Path) -> Result<(Connection, PathBuf), rusqlite::Error> {
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

/// Read history anchored to SNSS URLs, collecting navigation trees connected
/// to anchor URLs for each tab.
fn read_anchored_history(
    history_path: &Path,
    tab_anchors: &[(i32, Vec<String>)],
) -> Result<HashMap<i32, Vec<VisitEntry>>, rusqlite::Error> {
    let (conn, tmp_path) = open_history_db(history_path)?;

    let mut map: HashMap<i32, Vec<VisitEntry>> = HashMap::new();

    for (tab_id, anchor_urls) in tab_anchors {
        if let Ok(visits) = collect_tab_visits(&conn, *tab_id, anchor_urls)
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

/// Collect visits for a tab by walking backward from SNSS anchor URLs to find
/// navigation tree roots, then forward to collect complete trees. Uses temporal
/// anchoring (latest anchor visit_time) to guard against tab_id reuse across
/// browser sessions. Includes unannotated intermediaries (e.g. redirects) but
/// excludes visits annotated for a different tab and navigation trees not
/// connected to any anchor URL.
fn collect_tab_visits(
    conn: &Connection,
    tab_id: i32,
    anchor_urls: &[String],
) -> Result<Vec<VisitEntry>, rusqlite::Error> {
    if anchor_urls.is_empty() {
        return Ok(Vec::new());
    }

    // Step 1: Find the latest visit_time for any anchor URL on this tab_id.
    let url_placeholders: String = anchor_urls
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let anchor_sql = format!(
        "SELECT MAX(v.visit_time)
        FROM visits v
        JOIN urls u ON u.id = v.url
        JOIN context_annotations ca ON ca.visit_id = v.id
        WHERE ca.tab_id = ? AND u.url IN ({url_placeholders})"
    );

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    params.push(Box::new(tab_id));
    for url in anchor_urls {
        params.push(Box::new(url.clone()));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let upper_bound: Option<i64> = conn
        .query_row(&anchor_sql, param_refs.as_slice(), |row| row.get(0))
        .ok()
        .flatten();

    let upper_bound = match upper_bound {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };

    // Step 2: Walk backward from anchor visits to find tree roots, then forward
    // to collect the full navigation trees including unannotated intermediaries.
    // The backward walk stops at cross-tab boundaries so we don't leak into
    // other tabs' trees. Only trees containing an anchor URL are included.
    let numbered_placeholders: String = (0..anchor_urls.len())
        .map(|i| format!("?{}", i + 3))
        .collect::<Vec<_>>()
        .join(",");

    let chain_sql = format!(
        "WITH RECURSIVE
            anchor_visits(vid) AS (
                SELECT v.id
                FROM visits v
                JOIN urls u ON u.id = v.url
                JOIN context_annotations ca ON ca.visit_id = v.id
                WHERE ca.tab_id = ?1 AND u.url IN ({numbered_placeholders})
                  AND v.visit_time <= ?2
            ),
            -- Walk backward from anchors, stopping at cross-tab boundaries
            backward(vid) AS (
                SELECT vid FROM anchor_visits
                UNION
                SELECT v.from_visit
                FROM backward b
                JOIN visits v ON v.id = b.vid
                WHERE v.from_visit != 0
                  AND NOT EXISTS (
                    SELECT 1 FROM context_annotations ca
                    WHERE ca.visit_id = v.from_visit AND ca.tab_id != ?1
                  )
            ),
            -- Roots: backward visits that are tree roots or at cross-tab boundaries
            roots(vid) AS (
                SELECT DISTINCT v.id FROM backward b
                JOIN visits v ON v.id = b.vid
                WHERE v.from_visit = 0
                   OR v.from_visit NOT IN (SELECT vid FROM backward)
            ),
            -- Seed forward walk from both roots and anchor visits (anchors are
            -- re-seeded in case a cross-tab visit blocks the path from root)
            seeds(vid) AS (
                SELECT vid FROM roots
                UNION
                SELECT vid FROM anchor_visits
            ),
            -- Walk forward, including same-tab and unannotated visits only
            forward(vid) AS (
                SELECT vid FROM seeds
                UNION
                SELECT v.id
                FROM forward f
                JOIN visits v ON v.from_visit = f.vid
                WHERE v.visit_time <= ?2
                  AND NOT EXISTS (
                    SELECT 1 FROM context_annotations ca
                    WHERE ca.visit_id = v.id AND ca.tab_id != ?1
                  )
            )
        SELECT u.url, u.title, v.visit_time, fv_url.url AS from_url
        FROM forward f
        JOIN visits v ON v.id = f.vid
        JOIN urls u ON u.id = v.url
        LEFT JOIN visits fv ON fv.id = v.from_visit AND v.from_visit != 0
        LEFT JOIN urls fv_url ON fv_url.id = fv.url
        ORDER BY v.visit_time ASC"
    );

    let mut chain_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    chain_params.push(Box::new(tab_id));
    chain_params.push(Box::new(upper_bound));
    for url in anchor_urls {
        chain_params.push(Box::new(url.clone()));
    }
    let chain_param_refs: Vec<&dyn rusqlite::types::ToSql> =
        chain_params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&chain_sql)?;
    let mut rows = stmt.query(chain_param_refs.as_slice())?;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE urls (
                id INTEGER PRIMARY KEY,
                url TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE visits (
                id INTEGER PRIMARY KEY,
                url INTEGER NOT NULL REFERENCES urls(id),
                visit_time INTEGER NOT NULL,
                from_visit INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE context_annotations (
                visit_id INTEGER PRIMARY KEY REFERENCES visits(id),
                tab_id INTEGER NOT NULL
            );",
        )
        .unwrap();
        conn
    }

    fn insert_url(conn: &Connection, id: i64, url: &str, title: &str) {
        conn.execute(
            "INSERT INTO urls (id, url, title) VALUES (?, ?, ?)",
            rusqlite::params![id, url, title],
        )
        .unwrap();
    }

    fn insert_visit(conn: &Connection, id: i64, url_id: i64, time: i64, from_visit: i64) {
        conn.execute(
            "INSERT INTO visits (id, url, visit_time, from_visit) VALUES (?, ?, ?, ?)",
            rusqlite::params![id, url_id, time, from_visit],
        )
        .unwrap();
    }

    fn annotate(conn: &Connection, visit_id: i64, tab_id: i32) {
        conn.execute(
            "INSERT INTO context_annotations (visit_id, tab_id) VALUES (?, ?)",
            rusqlite::params![visit_id, tab_id],
        )
        .unwrap();
    }

    #[test]
    fn walks_backward_to_root_and_forward_through_chain() {
        let conn = create_test_db();
        // Linear chain: A -> B -> C (anchor)
        insert_url(&conn, 1, "https://a.example", "A");
        insert_url(&conn, 2, "https://b.example", "B");
        insert_url(&conn, 3, "https://c.example", "C");

        insert_visit(&conn, 10, 1, 100, 0);
        insert_visit(&conn, 11, 2, 200, 10);
        insert_visit(&conn, 12, 3, 300, 11);

        annotate(&conn, 10, 42);
        annotate(&conn, 11, 42);
        annotate(&conn, 12, 42);

        let visits = collect_tab_visits(&conn, 42, &["https://c.example".into()]).unwrap();

        let urls: Vec<_> = visits.iter().map(|v| v.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://a.example",
                "https://b.example",
                "https://c.example"
            ]
        );
        assert_eq!(visits[0].from_url, None);
        assert_eq!(visits[1].from_url.as_deref(), Some("https://a.example"));
        assert_eq!(visits[2].from_url.as_deref(), Some("https://b.example"));
    }

    #[test]
    fn includes_branching_descendants_for_same_tab() {
        let conn = create_test_db();
        // Tree: A -> B -> C, A -> D (branch from same root)
        // Anchor is D (latest visit), so temporal bound captures all visits.
        insert_url(&conn, 1, "https://root.example", "Root");
        insert_url(&conn, 2, "https://b.example", "B");
        insert_url(&conn, 3, "https://c.example", "C");
        insert_url(&conn, 4, "https://d.example", "D");

        insert_visit(&conn, 10, 1, 100, 0);
        insert_visit(&conn, 11, 2, 200, 10);
        insert_visit(&conn, 12, 3, 300, 11);
        insert_visit(&conn, 13, 4, 400, 10); // branch from root

        for v in [10, 11, 12, 13] {
            annotate(&conn, v, 55);
        }

        let visits = collect_tab_visits(&conn, 55, &["https://d.example".into()]).unwrap();

        let urls: Vec<_> = visits.iter().map(|v| v.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://root.example",
                "https://b.example",
                "https://c.example",
                "https://d.example",
            ]
        );
    }

    #[test]
    fn excludes_visits_annotated_for_different_tab() {
        let conn = create_test_db();
        // A -> B (tab 42), A -> C (tab 99, different tab branching from same root)
        insert_url(&conn, 1, "https://root.example", "Root");
        insert_url(&conn, 2, "https://same-tab.example", "Same");
        insert_url(&conn, 3, "https://other-tab.example", "Other");

        insert_visit(&conn, 10, 1, 100, 0);
        insert_visit(&conn, 11, 2, 200, 10);
        insert_visit(&conn, 12, 3, 300, 10); // branches from root but different tab

        annotate(&conn, 10, 42);
        annotate(&conn, 11, 42);
        annotate(&conn, 12, 99); // different tab

        let visits = collect_tab_visits(&conn, 42, &["https://same-tab.example".into()]).unwrap();

        let urls: Vec<_> = visits.iter().map(|v| v.url.as_str()).collect();
        assert_eq!(
            urls,
            vec!["https://root.example", "https://same-tab.example"]
        );
    }

    #[test]
    fn includes_unannotated_redirect_intermediaries() {
        let conn = create_test_db();
        // A -> redirect (no annotation) -> B
        insert_url(&conn, 1, "https://a.example", "A");
        insert_url(&conn, 2, "https://redirect.example", "Redirect");
        insert_url(&conn, 3, "https://b.example", "B");

        insert_visit(&conn, 10, 1, 100, 0);
        insert_visit(&conn, 11, 2, 200, 10); // no annotation
        insert_visit(&conn, 12, 3, 300, 11);

        annotate(&conn, 10, 42);
        // visit 11 deliberately has no annotation
        annotate(&conn, 12, 42);

        let visits = collect_tab_visits(&conn, 42, &["https://b.example".into()]).unwrap();

        let urls: Vec<_> = visits.iter().map(|v| v.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://a.example",
                "https://redirect.example",
                "https://b.example"
            ]
        );
    }

    #[test]
    fn returns_empty_when_no_anchor_matches() {
        let conn = create_test_db();
        insert_url(&conn, 1, "https://a.example", "A");
        insert_visit(&conn, 10, 1, 100, 0);
        annotate(&conn, 10, 42);

        let visits =
            collect_tab_visits(&conn, 42, &["https://nonexistent.example".into()]).unwrap();

        assert!(visits.is_empty());
    }

    #[test]
    fn excludes_unconnected_trees_for_same_tab() {
        let conn = create_test_db();
        // Two disconnected trees in the same tab. Only the tree containing
        // the anchor URL should be included.
        insert_url(&conn, 1, "https://typed1.example", "Typed1");
        insert_url(&conn, 2, "https://clicked.example", "Clicked");
        insert_url(&conn, 3, "https://typed2.example", "Typed2");
        insert_url(&conn, 4, "https://anchor.example", "Anchor");

        insert_visit(&conn, 10, 1, 100, 0); // typed (tree 1 root)
        insert_visit(&conn, 11, 2, 200, 10); // click from typed1 (tree 1)
        insert_visit(&conn, 12, 3, 300, 0); // typed (tree 2 root)
        insert_visit(&conn, 13, 4, 400, 12); // click from typed2 (tree 2, anchor)

        for v in [10, 11, 12, 13] {
            annotate(&conn, v, 42);
        }

        let visits = collect_tab_visits(&conn, 42, &["https://anchor.example".into()]).unwrap();

        let urls: Vec<_> = visits.iter().map(|v| v.url.as_str()).collect();
        // Only tree 2 (containing the anchor) is included; tree 1 is excluded.
        assert_eq!(
            urls,
            vec!["https://typed2.example", "https://anchor.example"]
        );
    }

    #[test]
    fn includes_multiple_trees_when_both_contain_anchors() {
        let conn = create_test_db();
        // Two disconnected trees, both containing anchor URLs.
        insert_url(&conn, 1, "https://typed1.example", "Typed1");
        insert_url(&conn, 2, "https://anchor1.example", "Anchor1");
        insert_url(&conn, 3, "https://typed2.example", "Typed2");
        insert_url(&conn, 4, "https://anchor2.example", "Anchor2");

        insert_visit(&conn, 10, 1, 100, 0);
        insert_visit(&conn, 11, 2, 200, 10);
        insert_visit(&conn, 12, 3, 300, 0);
        insert_visit(&conn, 13, 4, 400, 12);

        for v in [10, 11, 12, 13] {
            annotate(&conn, v, 42);
        }

        let visits = collect_tab_visits(
            &conn,
            42,
            &[
                "https://anchor1.example".into(),
                "https://anchor2.example".into(),
            ],
        )
        .unwrap();

        let urls: Vec<_> = visits.iter().map(|v| v.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://typed1.example",
                "https://anchor1.example",
                "https://typed2.example",
                "https://anchor2.example",
            ]
        );
    }

    #[test]
    fn excludes_visits_after_temporal_anchor() {
        let conn = create_test_db();
        // Tab_id 42 reused in a later session. Anchor URL exists in the earlier session.
        // Visits after the anchor's time should be excluded.
        insert_url(&conn, 1, "https://old.example", "Old");
        insert_url(&conn, 2, "https://anchor.example", "Anchor");
        insert_url(&conn, 3, "https://future.example", "Future");

        insert_visit(&conn, 10, 1, 100, 0);
        insert_visit(&conn, 11, 2, 200, 10); // anchor
        insert_visit(&conn, 12, 3, 500, 0); // later session reusing same tab_id

        annotate(&conn, 10, 42);
        annotate(&conn, 11, 42);
        annotate(&conn, 12, 42); // same tab_id but future session

        let visits = collect_tab_visits(&conn, 42, &["https://anchor.example".into()]).unwrap();

        let urls: Vec<_> = visits.iter().map(|v| v.url.as_str()).collect();
        assert_eq!(urls, vec!["https://old.example", "https://anchor.example"]);
    }

    #[test]
    fn anchors_to_latest_matching_visit() {
        let conn = create_test_db();
        // Anchor URL appears in two trees; both are connected to anchors,
        // and temporal bound uses the latest match to capture everything.
        insert_url(&conn, 1, "https://a.example", "A");
        insert_url(&conn, 2, "https://anchor.example", "Anchor");
        insert_url(&conn, 3, "https://c.example", "C");

        insert_visit(&conn, 10, 1, 100, 0);
        insert_visit(&conn, 11, 2, 200, 10); // anchor, tree 1
        insert_visit(&conn, 12, 3, 300, 0);
        insert_visit(&conn, 13, 2, 400, 12); // anchor, tree 2

        for v in [10, 11, 12, 13] {
            annotate(&conn, v, 42);
        }

        let visits = collect_tab_visits(&conn, 42, &["https://anchor.example".into()]).unwrap();

        let urls: Vec<_> = visits.iter().map(|v| v.url.as_str()).collect();
        // Both trees included since both contain the anchor URL
        assert_eq!(
            urls,
            vec![
                "https://a.example",
                "https://anchor.example",
                "https://c.example",
                "https://anchor.example",
            ]
        );
    }
}
