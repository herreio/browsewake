use crate::browser::BrowserSource;
use crate::browser::paths::firefox_profile_dirs;
use crate::error::{BrowseWakeError, Result};
use crate::model::{BrowserKind, BrowserWindows, NavEntry, Tab, Window};
use std::fs;
use std::path::Path;

const MOZLZ4_MAGIC: &[u8] = b"mozLz40\0";

pub struct Firefox;

impl BrowserSource for Firefox {
    fn kind(&self) -> BrowserKind {
        BrowserKind::Firefox
    }

    fn available(&self) -> bool {
        firefox_profile_dirs().is_ok()
    }

    fn export_tabs(&self, _deep_history: bool) -> Result<BrowserWindows> {
        let profiles = firefox_profile_dirs()?;
        let mut all_windows = Vec::new();

        for profile in &profiles {
            let recovery = profile.join("sessionstore-backups/recovery.jsonlz4");
            if recovery.exists() {
                match read_session(&recovery) {
                    Ok(windows) => all_windows.extend(windows),
                    Err(e) => eprintln!("warning: failed to read {}: {e}", recovery.display()),
                }
            }
        }

        Ok(BrowserWindows {
            browser: BrowserKind::Firefox,
            windows: all_windows,
        })
    }
}

fn read_session(path: &Path) -> Result<Vec<Window>> {
    let data = fs::read(path)?;
    let json = decompress_mozlz4(&data)?;
    let session: serde_json::Value = serde_json::from_slice(&json)?;
    parse_session(&session)
}

fn decompress_mozlz4(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < MOZLZ4_MAGIC.len() + 4 {
        return Err(BrowseWakeError::MozLz4("file too small".into()));
    }
    if &data[..MOZLZ4_MAGIC.len()] != MOZLZ4_MAGIC {
        return Err(BrowseWakeError::MozLz4("invalid magic bytes".into()));
    }

    let size_offset = MOZLZ4_MAGIC.len();
    let uncompressed_size = u32::from_le_bytes([
        data[size_offset],
        data[size_offset + 1],
        data[size_offset + 2],
        data[size_offset + 3],
    ]) as usize;

    let compressed = &data[size_offset + 4..];
    lz4_flex::block::decompress(compressed, uncompressed_size)
        .map_err(|e| BrowseWakeError::Lz4(e.to_string()))
}

fn parse_session(session: &serde_json::Value) -> Result<Vec<Window>> {
    let json_windows = session["windows"]
        .as_array()
        .ok_or_else(|| BrowseWakeError::MozLz4("missing 'windows' array".into()))?;

    let mut windows = Vec::new();

    for json_window in json_windows {
        let window_tabs = match json_window["tabs"].as_array() {
            Some(t) => t,
            None => continue,
        };

        let mut tabs = Vec::new();

        for tab_value in window_tabs {
            let entries = match tab_value["entries"].as_array() {
                Some(e) => e,
                None => continue,
            };

            let current_index = tab_value["index"]
                .as_u64()
                .map(|i| (i as usize).saturating_sub(1));

            let history: Vec<NavEntry> = entries
                .iter()
                .enumerate()
                .map(|(i, entry)| NavEntry {
                    url: entry["url"].as_str().unwrap_or("").to_string(),
                    title: entry["title"].as_str().unwrap_or("").to_string(),
                    index: i,
                })
                .collect();

            let (url, title) = if let Some(idx) = current_index {
                if let Some(current) = history.get(idx) {
                    (current.url.clone(), current.title.clone())
                } else if let Some(last) = history.last() {
                    (last.url.clone(), last.title.clone())
                } else {
                    continue;
                }
            } else if let Some(last) = history.last() {
                (last.url.clone(), last.title.clone())
            } else {
                continue;
            };

            tabs.push(Tab {
                url,
                title,
                history,
                current_index,
                deep_history: Vec::new(),
                tab_id: None,
            });
        }

        if !tabs.is_empty() {
            windows.push(Window { tabs });
        }
    }

    Ok(windows)
}
