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
                    Err(e) => eprintln!(
                        "warning: failed to read Chrome session in {}: {e}",
                        profile.display()
                    ),
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
    parse_snss(&data)
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

// --- Custom SNSS parser (replaces snss crate) ---
// The snss crate incorrectly treats command ID 6 as a tab navigation entry,
// but Chrome uses ID 6 for SetTabExtensionAppID which has a different format.
// Only command ID 1 (UpdateTabNavigation) contains tab data.

const SNSS_MAGIC: &[u8] = b"SNSS";
const CMD_UPDATE_TAB_NAVIGATION: u8 = 1;

fn read_u16_le(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_i32_le(data: &[u8], offset: usize) -> Option<i32> {
    data.get(offset..offset + 4)
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

struct SnssTab {
    id: i32,
    index: i32,
    url: String,
    title: String,
}

fn parse_snss(data: &[u8]) -> Result<Vec<Tab>> {
    if data.len() < 8 || &data[..4] != SNSS_MAGIC {
        return Err(BrowseWakeError::Snss("invalid SNSS header".into()));
    }

    let mut offset = 8; // skip magic + version
    let mut tab_navs: HashMap<i32, Vec<(usize, String, String)>> = HashMap::new();

    while offset + 2 <= data.len() {
        let cmd_len = read_u16_le(data, offset).unwrap() as usize;
        offset += 2;

        if cmd_len == 0 || offset + cmd_len > data.len() {
            break;
        }

        let cmd_id = data[offset];
        if cmd_id == CMD_UPDATE_TAB_NAVIGATION {
            if let Some(tab) = parse_tab_command(&data[offset..offset + cmd_len]) {
                tab_navs
                    .entry(tab.id)
                    .or_default()
                    .push((tab.index as usize, tab.url, tab.title));
            }
        }

        offset += cmd_len;
    }

    build_tabs(tab_navs)
}

fn parse_tab_command(cmd: &[u8]) -> Option<SnssTab> {
    // Layout: u8 cmd_id, 4 bytes padding, i32 id, i32 index,
    //         u32 url_len, url (padded to 4), u32 title_char_count, title UTF-16LE (padded to 4)
    let mut p = 1 + 4; // skip cmd_id + padding

    let id = read_i32_le(cmd, p)?;
    p += 4;
    let index = read_i32_le(cmd, p)?;
    p += 4;

    let url_len = read_u32_le(cmd, p)? as usize;
    p += 4;
    if p + url_len > cmd.len() {
        return None;
    }
    let url = String::from_utf8_lossy(&cmd[p..p + url_len]).into_owned();
    p += (url_len + 3) & !3; // align to 4

    let title_clen = read_u32_le(cmd, p)? as usize;
    p += 4;
    let title_byte_len = title_clen * 2;
    if p + title_byte_len > cmd.len() {
        return None;
    }
    let title_u16: Vec<u16> = cmd[p..p + title_byte_len]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let title = String::from_utf16_lossy(&title_u16);

    Some(SnssTab {
        id,
        index,
        url,
        title,
    })
}

fn build_tabs(tab_navs: HashMap<i32, Vec<(usize, String, String)>>) -> Result<Vec<Tab>> {
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
