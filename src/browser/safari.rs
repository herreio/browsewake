#![cfg(target_os = "macos")]

use crate::browser::BrowserSource;
use crate::browser::paths::safari_data_dir;
use crate::error::{BrowseWakeError, Result};
use crate::model::{BrowserKind, BrowserTabs, Tab};
use rusqlite::Connection;
use std::process::Command;

pub struct Safari;

impl BrowserSource for Safari {
    fn kind(&self) -> BrowserKind {
        BrowserKind::Safari
    }

    fn available(&self) -> bool {
        safari_data_dir().is_ok()
    }

    fn export_tabs(&self) -> Result<BrowserTabs> {
        match read_safari_tabs() {
            Ok(tabs) => Ok(BrowserTabs {
                browser: BrowserKind::Safari,
                tabs,
            }),
            Err(_) => {
                // Fallback to JXA if SQLite fails (e.g., TCC restrictions)
                eprintln!("warning: SQLite access failed, trying JXA fallback");
                let tabs = read_safari_jxa()?;
                Ok(BrowserTabs {
                    browser: BrowserKind::Safari,
                    tabs,
                })
            }
        }
    }
}

fn read_safari_tabs() -> Result<Vec<Tab>> {
    let safari_dir = safari_data_dir()?;

    // Try CloudTabs.db first (has more reliable data)
    let cloud_tabs_db = safari_dir.join("CloudTabs.db");
    if cloud_tabs_db.exists() {
        if let Ok(tabs) = read_cloud_tabs_db(&cloud_tabs_db) {
            if !tabs.is_empty() {
                return Ok(tabs);
            }
        }
    }

    // Try BrowserState.db
    let state_db = safari_dir.join("BrowserState.db");
    if state_db.exists() {
        return read_browser_state_db(&state_db);
    }

    Err(BrowseWakeError::NoProfile("safari (no database found)".into()))
}

fn read_cloud_tabs_db(path: &std::path::Path) -> Result<Vec<Tab>> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare(
        "SELECT url, title FROM cloud_tabs ORDER BY device_name, position",
    )?;

    let tabs = stmt
        .query_map([], |row| {
            Ok(Tab {
                url: row.get(0)?,
                title: row.get::<_, String>(1).unwrap_or_default(),
                history: Vec::new(),
                current_index: None,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(tabs)
}

fn read_browser_state_db(path: &std::path::Path) -> Result<Vec<Tab>> {
    let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    // BrowserState.db schema varies across macOS versions; try known queries
    let queries = [
        "SELECT url, title FROM tabs",
        "SELECT current_url, title FROM tabs",
    ];

    for query in &queries {
        if let Ok(mut stmt) = conn.prepare(query) {
            let tabs: Vec<Tab> = stmt
                .query_map([], |row| {
                    Ok(Tab {
                        url: row.get(0)?,
                        title: row.get::<_, String>(1).unwrap_or_default(),
                        history: Vec::new(),
                        current_index: None,
                    })
                })
                .ok()
                .into_iter()
                .flatten()
                .filter_map(|r| r.ok())
                .collect();

            if !tabs.is_empty() {
                return Ok(tabs);
            }
        }
    }

    Err(BrowseWakeError::Other("could not read Safari BrowserState.db".into()))
}

fn read_safari_jxa() -> Result<Vec<Tab>> {
    let script = r#"
        var safari = Application("Safari");
        var tabs = [];
        safari.windows().forEach(function(win) {
            win.tabs().forEach(function(tab) {
                tabs.push({ url: tab.url(), title: tab.name() });
            });
        });
        JSON.stringify(tabs);
    "#;

    let output = Command::new("osascript")
        .args(["-l", "JavaScript", "-e", script])
        .output()
        .map_err(|e| BrowseWakeError::Other(format!("failed to run osascript: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BrowseWakeError::Other(format!("JXA failed: {stderr}")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let jxa_tabs: Vec<serde_json::Value> = serde_json::from_str(stdout.trim())?;

    let tabs = jxa_tabs
        .into_iter()
        .map(|v| Tab {
            url: v["url"].as_str().unwrap_or("").to_string(),
            title: v["title"].as_str().unwrap_or("").to_string(),
            history: Vec::new(),
            current_index: None,
        })
        .collect();

    Ok(tabs)
}
