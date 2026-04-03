use crate::error::{BrowseWakeError, Result};
use crate::model::{NavEntry, Tab};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const SNSS_MAGIC: &[u8] = b"SNSS";
// In Tabs_* files, command ID 1 is UpdateTabNavigation
const CMD_TABS_UPDATE_TAB_NAVIGATION: u8 = 1;
// In Session_* files, command ID 6 is UpdateTabNavigation
const CMD_SESSION_UPDATE_TAB_NAVIGATION: u8 = 6;

struct SnssTab {
    id: i32,
    index: i32,
    url: String,
    title: String,
}

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

/// Read tabs from Chromium-based browser session directories.
pub fn read_chromium_sessions(profiles: &[PathBuf], browser_name: &str) -> Result<Vec<Tab>> {
    let mut all_tabs = Vec::new();
    for profile in profiles {
        let sessions_dir = profile.join("Sessions");
        if sessions_dir.is_dir() {
            match read_session(&sessions_dir) {
                Ok(tabs) => all_tabs.extend(tabs),
                Err(e) => eprintln!(
                    "warning: failed to read {browser_name} session in {}: {e}",
                    profile.display()
                ),
            }
        }
    }
    Ok(all_tabs)
}

fn read_session(sessions_dir: &Path) -> Result<Vec<Tab>> {
    // Session files are live journals covering all windows and are most up-to-date.
    // Tabs files are periodic snapshots. Prefer Session files; fall back to Tabs.
    let mut tab_navs: HashMap<i32, Vec<(usize, String, String)>> = HashMap::new();

    if let Some(session_file) = find_latest_file(sessions_dir, "Session_*") {
        let data = fs::read(&session_file)?;
        collect_tab_navs(&data, CMD_SESSION_UPDATE_TAB_NAVIGATION, &mut tab_navs)?;
    }

    if tab_navs.is_empty() {
        if let Some(tabs_file) = find_latest_file(sessions_dir, "Tabs_*") {
            let data = fs::read(&tabs_file)?;
            collect_tab_navs(&data, CMD_TABS_UPDATE_TAB_NAVIGATION, &mut tab_navs)?;
        }
    }

    if tab_navs.is_empty() {
        return Err(BrowseWakeError::NoProfile("(no session files)".into()));
    }

    build_tabs(tab_navs)
}

fn find_latest_file(sessions_dir: &Path, prefix: &str) -> Option<PathBuf> {
    let pattern = sessions_dir.join(prefix).to_string_lossy().to_string();
    let mut files: Vec<_> = glob::glob(&pattern).ok()?.flatten().collect();

    files.sort_by_key(|f| {
        std::cmp::Reverse(
            f.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
        )
    });

    files.into_iter().next()
}

/// Extract tab navigation entries from an SNSS file into the shared map.
fn collect_tab_navs(
    data: &[u8],
    nav_cmd_id: u8,
    tab_navs: &mut HashMap<i32, Vec<(usize, String, String)>>,
) -> Result<()> {
    if data.len() < 8 || &data[..4] != SNSS_MAGIC {
        return Err(BrowseWakeError::Snss("invalid SNSS header".into()));
    }

    let mut offset = 8; // skip magic + version

    while offset + 2 <= data.len() {
        let cmd_len = read_u16_le(data, offset).unwrap() as usize;
        offset += 2;

        if cmd_len == 0 || offset + cmd_len > data.len() {
            break;
        }

        let cmd_id = data[offset];
        if cmd_id == nav_cmd_id {
            if let Some(tab) = parse_tab_command(&data[offset..offset + cmd_len]) {
                tab_navs
                    .entry(tab.id)
                    .or_default()
                    .push((tab.index as usize, tab.url, tab.title));
            }
        }

        offset += cmd_len;
    }

    Ok(())
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
