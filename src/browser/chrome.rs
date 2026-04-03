use crate::browser::BrowserSource;
use crate::browser::paths::chrome_profile_dirs;
use crate::error::{BrowseWakeError, Result};
use crate::model::{BrowserKind, BrowserTabs, NavEntry, Tab};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub struct Chrome;

impl BrowserSource for Chrome {
    fn kind(&self) -> BrowserKind {
        BrowserKind::Chrome
    }

    fn available(&self) -> bool {
        chrome_profile_dirs().is_ok()
    }

    fn export_tabs(&self) -> Result<BrowserTabs> {
        let profiles = chrome_profile_dirs()?;
        let mut all_tabs = Vec::new();

        for profile in &profiles {
            let sessions_dir = profile.join("Sessions");
            if sessions_dir.is_dir() {
                match read_chrome_session(&sessions_dir) {
                    Ok(tabs) => all_tabs.extend(tabs),
                    Err(e) => eprintln!("warning: failed to read Chrome session in {}: {e}", profile.display()),
                }
            }
        }

        Ok(BrowserTabs {
            browser: BrowserKind::Chrome,
            tabs: all_tabs,
        })
    }
}

fn read_chrome_session(sessions_dir: &Path) -> Result<Vec<Tab>> {
    let session_file = find_latest_session(sessions_dir)?;
    let data = fs::read(&session_file)?;
    let parsed = snss::parse(&data).map_err(|e| BrowseWakeError::Snss(format!("{e}")))?;
    parse_snss_session(&parsed)
}

fn find_latest_session(sessions_dir: &Path) -> Result<std::path::PathBuf> {
    let pattern = sessions_dir.join("Tabs_*").to_string_lossy().to_string();
    let mut files: Vec<_> = glob::glob(&pattern)
        .map_err(|e| BrowseWakeError::Other(e.to_string()))?
        .flatten()
        .collect();

    if files.is_empty() {
        let pattern = sessions_dir.join("Session_*").to_string_lossy().to_string();
        files = glob::glob(&pattern)
            .map_err(|e| BrowseWakeError::Other(e.to_string()))?
            .flatten()
            .collect();
    }

    if files.is_empty() {
        return Err(BrowseWakeError::NoProfile("chrome (no session files)".into()));
    }

    files.sort_by_key(|f| {
        std::cmp::Reverse(
            f.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
        )
    });

    Ok(files.into_iter().next().unwrap())
}

fn parse_snss_session(session: &snss::SNSS) -> Result<Vec<Tab>> {
    // Group tab navigation entries by tab ID
    let mut tab_navs: HashMap<i32, Vec<(usize, String, String)>> = HashMap::new();

    for cmd in &session.commands {
        if let snss::Content::Tab(tab) = &cmd.content {
            tab_navs
                .entry(tab.id)
                .or_default()
                .push((tab.index as usize, tab.url.clone(), tab.title.clone()));
        }
    }

    let mut tabs = Vec::new();

    for (_tab_id, mut navs) in tab_navs {
        navs.sort_by_key(|(idx, _, _)| *idx);
        navs.dedup_by_key(|(idx, _, _)| *idx);

        let history: Vec<NavEntry> = navs
            .iter()
            .map(|(idx, url, title)| NavEntry {
                url: url.clone(),
                title: title.clone(),
                index: *idx,
            })
            .collect();

        // Use the highest index as the current entry (best heuristic available)
        let current_index = history.last().map(|e| e.index);

        let (url, title) = history
            .last()
            .map(|e| (e.url.clone(), e.title.clone()))
            .unwrap_or_default();

        if !url.is_empty() {
            tabs.push(Tab {
                url,
                title,
                history,
                current_index,
            });
        }
    }

    Ok(tabs)
}
