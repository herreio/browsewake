#![cfg(target_os = "macos")]

use crate::browser::BrowserSource;
use crate::error::{BrowseWakeError, Result};
use crate::model::{BrowserKind, BrowserWindows, Tab, Window};
use std::process::Command;

pub struct Safari;

impl BrowserSource for Safari {
    fn kind(&self) -> BrowserKind {
        BrowserKind::Safari
    }

    fn available(&self) -> bool {
        std::path::Path::new("/Applications/Safari.app").exists()
    }

    fn export_tabs(&self, _deep_history: bool) -> Result<BrowserWindows> {
        let windows = read_safari_jxa()?;
        Ok(BrowserWindows {
            browser: BrowserKind::Safari,
            windows,
        })
    }
}

fn read_safari_jxa() -> Result<Vec<Window>> {
    let script = r#"
        var safari = Application("Safari");
        var windows = [];
        safari.windows().forEach(function(win) {
            var tabs = [];
            win.tabs().forEach(function(tab) {
                tabs.push({ url: tab.url(), title: tab.name() });
            });
            windows.push(tabs);
        });
        JSON.stringify(windows);
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
    let jxa_windows: Vec<Vec<serde_json::Value>> = serde_json::from_str(stdout.trim())?;

    let windows = jxa_windows
        .into_iter()
        .map(|jxa_tabs| {
            let tabs = jxa_tabs
                .into_iter()
                .map(|v| Tab {
                    url: v["url"].as_str().unwrap_or("").to_string(),
                    title: v["title"].as_str().unwrap_or("").to_string(),
                    history: Vec::new(),
                    current_index: None,
                    deep_history: Vec::new(),
                    tab_id: None,
                })
                .collect();
            Window { tabs }
        })
        .filter(|w: &Window| !w.tabs.is_empty())
        .collect();

    Ok(windows)
}
