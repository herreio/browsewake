use crate::error::{BrowseWakeError, Result};
use crate::model::{NavEntry, Tab, Window};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const SNSS_MAGIC: &[u8] = b"SNSS";
// In Tabs_* files, command ID 1 is UpdateTabNavigation
const CMD_TABS_UPDATE_TAB_NAVIGATION: u8 = 1;
// In Session_* files, command ID 0 is SetTabWindow, 2 is SetTabIndexInWindow,
// 5/11 prune the navigation path, 6 updates a navigation entry, and 7 stores
// the selected navigation index.
const CMD_SESSION_SET_TAB_WINDOW: u8 = 0;
const CMD_SESSION_SET_TAB_INDEX_IN_WINDOW: u8 = 2;
const CMD_SESSION_TAB_NAVIGATION_PATH_PRUNED_FROM_BACK: u8 = 5;
const CMD_SESSION_UPDATE_TAB_NAVIGATION: u8 = 6;
const CMD_SESSION_SET_SELECTED_NAVIGATION_INDEX: u8 = 7;
const CMD_SESSION_TAB_NAVIGATION_PATH_PRUNED_FROM_FRONT: u8 = 11;

type TabNav = (usize, String, String);

struct SnssTab {
    id: i32,
    index: i32,
    url: String,
    title: String,
}

#[derive(Default)]
struct TabState {
    window_id: Option<i32>,
    visual_index: Option<i32>,
    selected_navigation_index: Option<i32>,
    first_seen_order: usize,
    navs: Vec<TabNav>,
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

/// Read tabs from Chromium-based browser session directories, grouped by window.
pub fn read_chromium_sessions(profiles: &[PathBuf], browser_name: &str) -> Result<Vec<Window>> {
    let mut all_windows = Vec::new();
    for profile in profiles {
        let sessions_dir = profile.join("Sessions");
        if sessions_dir.is_dir() {
            match read_session(&sessions_dir) {
                Ok(windows) => all_windows.extend(windows),
                Err(e) => eprintln!(
                    "warning: failed to read {browser_name} session in {}: {e}",
                    profile.display()
                ),
            }
        }
    }
    Ok(all_windows)
}

fn read_session(sessions_dir: &Path) -> Result<Vec<Window>> {
    // Session files are live journals covering all windows and are most up-to-date.
    // They also contain SetTabWindow commands for window grouping.
    // Tabs files are periodic snapshots without window info. Fall back to them.
    if let Some(session_file) = find_latest_file(sessions_dir, "Session_*") {
        let data = fs::read(&session_file)?;
        let windows = parse_session_file(&data)?;
        if !windows.is_empty() {
            return Ok(windows);
        }
    }

    if let Some(tabs_file) = find_latest_file(sessions_dir, "Tabs_*") {
        let data = fs::read(&tabs_file)?;
        let tabs = parse_tabs_file(&data)?;
        if !tabs.is_empty() {
            return Ok(vec![Window { tabs }]);
        }
    }

    Err(BrowseWakeError::NoProfile("(no session files)".into()))
}

/// Parse a Session_* file, extracting both tab-to-window mappings and navigation entries.
fn parse_session_file(data: &[u8]) -> Result<Vec<Window>> {
    if data.len() < 8 || &data[..4] != SNSS_MAGIC {
        return Err(BrowseWakeError::Snss("invalid SNSS header".into()));
    }

    let mut tab_states: HashMap<i32, TabState> = HashMap::new();
    let mut next_seen_order = 0;
    let mut offset = 8;

    while offset + 2 <= data.len() {
        let cmd_len = read_u16_le(data, offset).unwrap() as usize;
        offset += 2;

        if cmd_len == 0 || offset + cmd_len > data.len() {
            break;
        }

        let cmd = &data[offset..offset + cmd_len];
        let cmd_id = cmd[0];

        match cmd_id {
            CMD_SESSION_SET_TAB_WINDOW if cmd.len() >= 9 => {
                // SetTabWindow: u8 cmd_id, i32 window_id, i32 tab_id
                let window_id = read_i32_le(cmd, 1).unwrap();
                let tab_id = read_i32_le(cmd, 5).unwrap();
                get_or_insert_tab_state(&mut tab_states, tab_id, &mut next_seen_order).window_id =
                    Some(window_id);
            }
            CMD_SESSION_SET_TAB_INDEX_IN_WINDOW if cmd.len() >= 9 => {
                // SetTabIndexInWindow: u8 cmd_id, i32 tab_id, i32 visual_index
                let tab_id = read_i32_le(cmd, 1).unwrap();
                let visual_index = read_i32_le(cmd, 5).unwrap();
                get_or_insert_tab_state(&mut tab_states, tab_id, &mut next_seen_order)
                    .visual_index = Some(visual_index);
            }
            CMD_SESSION_TAB_NAVIGATION_PATH_PRUNED_FROM_BACK if cmd.len() >= 9 => {
                // TabNavigationPathPrunedFromBack: u8 cmd_id, i32 tab_id, i32 index
                let tab_id = read_i32_le(cmd, 1).unwrap();
                let index = read_i32_le(cmd, 5).unwrap();
                prune_navigation_path_from_back(
                    get_or_insert_tab_state(&mut tab_states, tab_id, &mut next_seen_order),
                    index,
                );
            }
            CMD_SESSION_UPDATE_TAB_NAVIGATION => {
                if let Some(tab) = parse_tab_command(cmd) {
                    update_navigation_entry(
                        get_or_insert_tab_state(&mut tab_states, tab.id, &mut next_seen_order),
                        tab.index,
                        tab.url,
                        tab.title,
                    );
                }
            }
            CMD_SESSION_SET_SELECTED_NAVIGATION_INDEX if cmd.len() >= 9 => {
                // SetSelectedNavigationIndex: u8 cmd_id, i32 tab_id, i32 index
                let tab_id = read_i32_le(cmd, 1).unwrap();
                let index = read_i32_le(cmd, 5).unwrap();
                get_or_insert_tab_state(&mut tab_states, tab_id, &mut next_seen_order)
                    .selected_navigation_index = Some(index);
            }
            CMD_SESSION_TAB_NAVIGATION_PATH_PRUNED_FROM_FRONT if cmd.len() >= 9 => {
                // TabNavigationPathPrunedFromFront: u8 cmd_id, i32 tab_id, i32 count
                let tab_id = read_i32_le(cmd, 1).unwrap();
                let count = read_i32_le(cmd, 5).unwrap();
                prune_navigation_path_from_front(
                    get_or_insert_tab_state(&mut tab_states, tab_id, &mut next_seen_order),
                    count,
                );
            }
            _ => {}
        }

        offset += cmd_len;
    }

    // Group tabs by window while preserving the browser's visual tab ordering.
    let mut window_tabs: HashMap<i32, Vec<(i32, TabState)>> = HashMap::new();
    let mut unassigned = Vec::new();

    for (tab_id, state) in tab_states {
        if let Some(window_id) = state.window_id {
            window_tabs
                .entry(window_id)
                .or_default()
                .push((tab_id, state));
        } else {
            unassigned.push((tab_id, state));
        }
    }

    let mut windows = Vec::new();

    // Build windows in sorted order for deterministic output
    let mut window_ids: Vec<_> = window_tabs.keys().copied().collect();
    window_ids.sort();

    for window_id in window_ids {
        let tab_states = window_tabs.remove(&window_id).unwrap();
        let tabs = build_tabs(tab_states)?;
        if !tabs.is_empty() {
            windows.push(Window { tabs });
        }
    }

    // Put any unassigned tabs in their own window
    if !unassigned.is_empty() {
        let tabs = build_tabs(unassigned)?;
        if !tabs.is_empty() {
            windows.push(Window { tabs });
        }
    }

    Ok(windows)
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

fn parse_tabs_file(data: &[u8]) -> Result<Vec<Tab>> {
    let mut tab_states: HashMap<i32, TabState> = HashMap::new();
    let mut next_seen_order = 0;

    if data.len() < 8 || &data[..4] != SNSS_MAGIC {
        return Err(BrowseWakeError::Snss("invalid SNSS header".into()));
    }

    let mut offset = 8;

    while offset + 2 <= data.len() {
        let cmd_len = read_u16_le(data, offset).unwrap() as usize;
        offset += 2;

        if cmd_len == 0 || offset + cmd_len > data.len() {
            break;
        }

        let cmd_id = data[offset];
        if cmd_id == CMD_TABS_UPDATE_TAB_NAVIGATION
            && let Some(tab) = parse_tab_command(&data[offset..offset + cmd_len])
        {
            update_navigation_entry(
                get_or_insert_tab_state(&mut tab_states, tab.id, &mut next_seen_order),
                tab.index,
                tab.url,
                tab.title,
            );
        }

        offset += cmd_len;
    }

    build_tabs(tab_states.into_iter().collect())
}

fn get_or_insert_tab_state<'a>(
    tab_states: &'a mut HashMap<i32, TabState>,
    tab_id: i32,
    next_seen_order: &mut usize,
) -> &'a mut TabState {
    tab_states.entry(tab_id).or_insert_with(|| {
        let first_seen_order = *next_seen_order;
        *next_seen_order += 1;
        TabState {
            first_seen_order,
            ..TabState::default()
        }
    })
}

fn update_navigation_entry(tab_state: &mut TabState, index: i32, url: String, title: String) {
    if index < 0 {
        return;
    }

    let index = index as usize;
    if let Some(nav) = tab_state.navs.iter_mut().find(|(idx, _, _)| *idx == index) {
        *nav = (index, url, title);
    } else {
        tab_state.navs.push((index, url, title));
    }
}

fn prune_navigation_path_from_back(tab_state: &mut TabState, index: i32) {
    if index < 0 {
        return;
    }

    let cutoff = index as usize;
    tab_state.navs.retain(|(idx, _, _)| *idx < cutoff);

    if tab_state
        .selected_navigation_index
        .is_some_and(|selected| selected >= index)
    {
        tab_state.selected_navigation_index = None;
    }
}

fn prune_navigation_path_from_front(tab_state: &mut TabState, count: i32) {
    if count <= 0 {
        return;
    }

    let count = count as usize;
    tab_state.navs = tab_state
        .navs
        .drain(..)
        .filter_map(|(idx, url, title)| (idx >= count).then_some((idx - count, url, title)))
        .collect();

    if let Some(selected) = tab_state.selected_navigation_index {
        tab_state.selected_navigation_index =
            (selected - count as i32 >= 0).then_some(selected - count as i32);
    }
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

fn build_tabs(mut tab_states: Vec<(i32, TabState)>) -> Result<Vec<Tab>> {
    tab_states.sort_by_key(|(tab_id, state)| {
        (
            state.visual_index.unwrap_or(i32::MAX),
            state.first_seen_order,
            *tab_id,
        )
    });

    let mut tabs = Vec::new();

    for (_tab_id, mut state) in tab_states {
        let navs = &mut state.navs;
        navs.sort_by_key(|(idx, _, _)| *idx);

        let history: Vec<NavEntry> = navs
            .iter()
            .map(|(idx, url, title)| NavEntry {
                url: url.clone(),
                title: title.clone(),
                index: *idx,
            })
            .collect();

        let current_index = state
            .selected_navigation_index
            .and_then(|idx| {
                let idx = idx as usize;
                history.iter().find(|e| e.index == idx).map(|e| e.index)
            })
            .or_else(|| history.last().map(|e| e.index));

        let (url, title) = current_index
            .and_then(|ci| history.iter().find(|e| e.index == ci))
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

#[cfg(test)]
mod tests {
    use super::{parse_session_file, parse_tabs_file};

    fn command(cmd: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + cmd.len());
        out.extend_from_slice(&(cmd.len() as u16).to_le_bytes());
        out.extend_from_slice(cmd);
        out
    }

    fn session_file(commands: Vec<Vec<u8>>) -> Vec<u8> {
        let mut out = Vec::from(*b"SNSS");
        out.extend_from_slice(&[0, 0, 0, 0]);
        for cmd in commands {
            out.extend_from_slice(&cmd);
        }
        out
    }

    fn set_tab_window(window_id: i32, tab_id: i32) -> Vec<u8> {
        let mut cmd = vec![0];
        cmd.extend_from_slice(&window_id.to_le_bytes());
        cmd.extend_from_slice(&tab_id.to_le_bytes());
        command(&cmd)
    }

    fn set_tab_index(tab_id: i32, visual_index: i32) -> Vec<u8> {
        let mut cmd = vec![2];
        cmd.extend_from_slice(&tab_id.to_le_bytes());
        cmd.extend_from_slice(&visual_index.to_le_bytes());
        command(&cmd)
    }

    fn prune_from_back(tab_id: i32, index: i32) -> Vec<u8> {
        let mut cmd = vec![5];
        cmd.extend_from_slice(&tab_id.to_le_bytes());
        cmd.extend_from_slice(&index.to_le_bytes());
        command(&cmd)
    }

    fn set_selected_navigation_index(tab_id: i32, index: i32) -> Vec<u8> {
        let mut cmd = vec![7];
        cmd.extend_from_slice(&tab_id.to_le_bytes());
        cmd.extend_from_slice(&index.to_le_bytes());
        command(&cmd)
    }

    fn prune_from_front(tab_id: i32, count: i32) -> Vec<u8> {
        let mut cmd = vec![11];
        cmd.extend_from_slice(&tab_id.to_le_bytes());
        cmd.extend_from_slice(&count.to_le_bytes());
        command(&cmd)
    }

    fn update_tab_navigation(
        cmd_id: u8,
        tab_id: i32,
        index: i32,
        url: &str,
        title: &str,
    ) -> Vec<u8> {
        let mut cmd = vec![cmd_id];
        cmd.extend_from_slice(&[0, 0, 0, 0]);
        cmd.extend_from_slice(&tab_id.to_le_bytes());
        cmd.extend_from_slice(&index.to_le_bytes());
        cmd.extend_from_slice(&(url.len() as u32).to_le_bytes());
        cmd.extend_from_slice(url.as_bytes());
        let url_padding = (4 - (url.len() % 4)) % 4;
        cmd.extend(std::iter::repeat_n(0, url_padding));

        let title_u16: Vec<u16> = title.encode_utf16().collect();
        cmd.extend_from_slice(&(title_u16.len() as u32).to_le_bytes());
        for ch in title_u16 {
            cmd.extend_from_slice(&ch.to_le_bytes());
        }
        let title_byte_len = title.encode_utf16().count() * 2;
        let title_padding = (4 - (title_byte_len % 4)) % 4;
        cmd.extend(std::iter::repeat_n(0, title_padding));

        command(&cmd)
    }

    #[test]
    fn session_tabs_follow_visual_index_order() {
        let data = session_file(vec![
            set_tab_window(7, 300),
            set_tab_window(7, 100),
            update_tab_navigation(6, 300, 0, "https://third.example", "Third"),
            update_tab_navigation(6, 100, 0, "https://first.example", "First"),
            set_tab_index(300, 2),
            set_tab_index(100, 0),
            set_tab_window(7, 200),
            update_tab_navigation(6, 200, 0, "https://second.example", "Second"),
            set_tab_index(200, 1),
        ]);

        let windows = parse_session_file(&data).unwrap();
        let urls: Vec<_> = windows[0].tabs.iter().map(|tab| tab.url.as_str()).collect();

        assert_eq!(
            urls,
            vec![
                "https://first.example",
                "https://second.example",
                "https://third.example"
            ]
        );
    }

    #[test]
    fn session_tabs_fall_back_to_first_seen_order_without_visual_index() {
        let data = session_file(vec![
            set_tab_window(5, 20),
            update_tab_navigation(6, 20, 0, "https://second.example", "Second"),
            set_tab_window(5, 10),
            update_tab_navigation(6, 10, 0, "https://first.example", "First"),
        ]);

        let windows = parse_session_file(&data).unwrap();
        let urls: Vec<_> = windows[0].tabs.iter().map(|tab| tab.url.as_str()).collect();

        assert_eq!(
            urls,
            vec!["https://second.example", "https://first.example"]
        );
    }

    #[test]
    fn tabs_file_follows_first_seen_tab_order() {
        let data = session_file(vec![
            update_tab_navigation(1, 50, 0, "https://two.example", "Two"),
            update_tab_navigation(1, 10, 0, "https://one.example", "One"),
            update_tab_navigation(1, 90, 0, "https://three.example", "Three"),
        ]);

        let tabs = parse_tabs_file(&data).unwrap();
        let urls: Vec<_> = tabs.iter().map(|tab| tab.url.as_str()).collect();

        assert_eq!(
            urls,
            vec![
                "https://two.example",
                "https://one.example",
                "https://three.example"
            ]
        );
    }

    #[test]
    fn tab_history_remains_sorted_by_navigation_index() {
        let data = session_file(vec![
            set_tab_window(1, 42),
            update_tab_navigation(6, 42, 2, "https://third.example", "Third"),
            update_tab_navigation(6, 42, 0, "https://first.example", "First"),
            update_tab_navigation(6, 42, 1, "https://second.example", "Second"),
            set_selected_navigation_index(42, 2),
            set_tab_index(42, 0),
        ]);

        let windows = parse_session_file(&data).unwrap();
        let history_urls: Vec<_> = windows[0].tabs[0]
            .history
            .iter()
            .map(|entry| entry.url.as_str())
            .collect();

        assert_eq!(
            history_urls,
            vec![
                "https://first.example",
                "https://second.example",
                "https://third.example"
            ]
        );
        assert_eq!(windows[0].tabs[0].current_index, Some(2));
        assert_eq!(windows[0].tabs[0].url, "https://third.example");
    }

    #[test]
    fn session_prunes_old_entries_from_front_and_renumbers_history() {
        let data = session_file(vec![
            set_tab_window(1, 99),
            update_tab_navigation(6, 99, 3, "https://four.example", "Four"),
            update_tab_navigation(6, 99, 4, "https://five.example", "Five"),
            update_tab_navigation(6, 99, 5, "https://six.example", "Six"),
            prune_from_front(99, 3),
            set_selected_navigation_index(99, 2),
            set_tab_index(99, 0),
        ]);

        let windows = parse_session_file(&data).unwrap();
        let tab = &windows[0].tabs[0];
        let history: Vec<_> = tab.history.iter().map(|entry| entry.index).collect();
        let urls: Vec<_> = tab.history.iter().map(|entry| entry.url.as_str()).collect();

        assert_eq!(history, vec![0, 1, 2]);
        assert_eq!(
            urls,
            vec![
                "https://four.example",
                "https://five.example",
                "https://six.example"
            ]
        );
        assert_eq!(tab.current_index, Some(2));
        assert_eq!(tab.url, "https://six.example");
    }

    #[test]
    fn session_preserves_original_indices_when_history_starts_above_zero() {
        // Simulates entries 0-33 pruned in a previous session cycle — only 34+ are in the file.
        let data = session_file(vec![
            set_tab_window(1, 55),
            update_tab_navigation(6, 55, 34, "https://page34.example", "Page 34"),
            update_tab_navigation(6, 55, 35, "https://page35.example", "Page 35"),
            update_tab_navigation(6, 55, 36, "https://page36.example", "Page 36"),
            set_selected_navigation_index(55, 36),
            set_tab_index(55, 0),
        ]);

        let windows = parse_session_file(&data).unwrap();
        let tab = &windows[0].tabs[0];
        let indices: Vec<_> = tab.history.iter().map(|e| e.index).collect();

        assert_eq!(indices, vec![34, 35, 36]);
        assert_eq!(tab.current_index, Some(36));
        assert_eq!(tab.url, "https://page36.example");
    }

    #[test]
    fn session_prunes_forward_entries_from_back_and_keeps_selected_item() {
        let data = session_file(vec![
            set_tab_window(1, 77),
            update_tab_navigation(6, 77, 0, "https://one.example", "One"),
            update_tab_navigation(6, 77, 1, "https://two.example", "Two"),
            update_tab_navigation(6, 77, 2, "https://three.example", "Three"),
            update_tab_navigation(6, 77, 3, "https://four.example", "Four"),
            prune_from_back(77, 2),
            set_selected_navigation_index(77, 1),
            set_tab_index(77, 0),
        ]);

        let windows = parse_session_file(&data).unwrap();
        let tab = &windows[0].tabs[0];
        let urls: Vec<_> = tab.history.iter().map(|entry| entry.url.as_str()).collect();

        assert_eq!(urls, vec!["https://one.example", "https://two.example"]);
        assert_eq!(tab.current_index, Some(1));
        assert_eq!(tab.url, "https://two.example");
    }
}
