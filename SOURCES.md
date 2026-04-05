# Parser and Output Reference

This document records the data sources browsewake currently parses, the meaning of each output field, and the upstream references used to build or investigate those parsers. It is intended as the handoff point for future CLI development.

## Output Semantics

### JSON / compact JSON

- `browsers[]` contains one object per exported browser.
- `windows[]` contains the parsed windows for that browser.
- `tabs[]` contains the current `url` and `title` for each tab.
- `history[]` is the authoritative per-tab back/forward list when the browser's session format exposes it directly.
- `current_index` identifies the selected `history[]` entry when the session format stores it.
- `deep_history[]` is Chromium-only supplemental visit history reconstructed from the History DB. It is anchored to the current SNSS session state and can differ from the visible back/forward list.

### Text

- Mirrors the JSON structure in a readable layout.
- `History (N entries)` prints indexed navigation entries and marks the selected entry with `<-- current`.
- `Deep History (N visits)` prints Chromium visit records in chronological order with `from_url` when available.

### CSV

- One row per `history[]` entry, with tab fields repeated.
- Tabs without `history[]` still emit one row with empty history columns.
- `deep_history[]` is not exported in CSV.

## Firefox

### `recovery.jsonlz4`

- **Location:** `sessionstore-backups/recovery.jsonlz4` in each profile.
- **Role:** exact session restore source for windows, tabs, and per-tab back/forward history.
- **Format:** `mozLz40\0` magic + 4-byte little-endian uncompressed size + LZ4 block.
- **Fields used:** `windows[].tabs[].entries[]` for navigation entries and `windows[].tabs[].index` for the selected entry. Firefox stores the selected index as 1-based; browsewake normalizes it to 0-based.
- **Confidence:** high.
- **References:**
  - Mozilla source: [SessionStore.sys.mjs](https://searchfox.org/mozilla-central/source/browser/components/sessionstore/SessionStore.sys.mjs)
  - Mozilla search: [sessionstore references to `recovery.jsonlz4`](https://searchfox.org/mozilla-central/search?q=recovery.jsonlz4&path=browser/components/sessionstore)
  - Format background: [Bug 818587](https://bugzilla.mozilla.org/show_bug.cgi?id=818587)

## Chrome / Brave

### SNSS session files

- **Location:** `Sessions/Session_*` and `Sessions/Tabs_*` in each profile.
- **Role:** authoritative source for current windows, tab ordering, and restorable per-tab back/forward history.
- **Format:** `SNSS` magic + 4 reserved bytes + stream of 2-byte-length-prefixed commands.
- **Commands used by browsewake:**
  - `0` `SetTabWindow`
  - `2` `SetTabIndexInWindow`
  - `5` `TabNavigationPathPrunedFromBack`
  - `6` `UpdateTabNavigation`
  - `7` `SetSelectedNavigationIndex`
  - `11` `TabNavigationPathPrunedFromFront`
- **Tab nav payload layout (cmd 6):** tab_id (i32) + nav_index (i32) + url (4-byte length-prefixed UTF-8, padded to 4-byte boundary) + title (4-byte length-prefixed UTF-16LE, padded to 4-byte boundary).
- **Confidence:** high for current restorable session state.
- **References:**
  - Chromium source: [session_service_commands.cc](https://source.chromium.org/chromium/chromium/src/+/main:components/sessions/core/session_service_commands.cc)
  - Chromium source: [session_command.h](https://source.chromium.org/chromium/chromium/src/+/main:components/sessions/core/session_command.h)

### History SQLite DB (`--deep-history`)

- **Location:** `History` in each Chromium profile.
- **Role:** supplemental visit graph, not the canonical back/forward list.
- **Tables used:** `urls`, `visits`, `context_annotations`.
- **Fields used:** `visits.visit_time`, `visits.from_visit`, `urls.url`, `urls.title`, `context_annotations.tab_id`.
- **Timestamp format:** `visits.visit_time` is microseconds since 1601-01-01 (Windows FILETIME epoch). Browsewake outputs these values as Unix epoch seconds.
- **`from_visit` semantics:** `from_visit = 0` means a typed/bookmarked navigation root (no causal predecessor). Non-zero values form a forest of navigation trees.
- **Query strategy:** collect the current SNSS tab URLs as anchors, find matching visits for the same `tab_id`, walk backward to roots, then forward through same-tab or unannotated descendants to recover a causally connected visit tree.
- **Confidence:** medium.
- **Known limitations:**
  - `deep_history[]` can include causally connected visits that are not currently visible in the browser's back/forward UI.
  - `deep_history[]` can be empty even when SNSS `history[]` exists, if the History DB does not have matching annotated visits.
  - Redirect/intermediate visits can appear because the History DB is visit-oriented.
- **References:**
  - Chromium source: [history_types.h](https://source.chromium.org/chromium/chromium/src/+/main:components/history/core/browser/history_types.h)
  - Chromium source: [visit_annotations_database.cc](https://source.chromium.org/chromium/chromium/src/+/main:components/history/core/browser/visit_annotations_database.cc)
  - Chromium source: [page_transition_types.h](https://source.chromium.org/chromium/chromium/src/+/main:ui/base/page_transition_types.h)

## Safari

### Current implementation

- **Locations used:** `~/Library/Safari/CloudTabs.db`, `~/Library/Safari/BrowserState.db`
- **Known additional location:** `~/Library/Containers/com.apple.Safari/Data/Library/Safari/SafariTabs.db` (macOS Monterey+, sandboxed container). Not currently used by browsewake.
- **Fallback:** live JXA via `osascript -l JavaScript`
- **Role today:** current tabs only. Browsewake does not currently claim Safari per-tab back/forward history support.
- **Current parser behavior:**
  - `CloudTabs.db` is tried first for `url` / `title` pairs from the `cloud_tabs` table.
  - `BrowserState.db` is used as a fallback for current tab URLs/titles via the `tabs` table.
  - JXA is used when SQLite access fails, typically due to TCC/privacy restrictions.
- **Confidence:** medium for current-tab export, low for anything beyond that.

### Investigation references

- These references were used to investigate whether Safari may persist recoverable per-tab history, but browsewake does not currently rely on them for runtime behavior:
  - Apple Support: [Go back to pages you have already visited in Safari on Mac](https://support.apple.com/en-bw/guide/safari/ibrw1009/mac)
  - Apple Developer Docs: [WKBackForwardList currentItem](https://developer.apple.com/documentation/webkit/wkbackforwardlist/currentitem)
  - Apple Developer Docs: [WKWebView interactionState](https://developer.apple.com/documentation/webkit/wkwebview/interactionstate)
  - Reverse-engineered reference: [mac_apt Safari parser](https://github.com/ydkhatri/mac_apt/blob/master/plugins/safari.py)
  - Reverse-engineered reference: [BrowserState.db analysis](https://doubleblak.com/browserstate)
  - Local scripting dictionary inspection: `sdef /Applications/Safari.app`

### Safari limitations

- Safari's scripting interface exposes tab URL, title, and ordering, but not a back/forward list.
- Access to `~/Library/Safari` and `~/Library/Containers/com.apple.Safari/` is gated by macOS TCC (Transparency, Consent, and Control). The calling process needs Full Disk Access (FDA) in System Settings > Privacy & Security. Without FDA, `path.exists()` and `path.is_file()` can still return `true`, but `File::open()` and SQLite `Connection::open` will fail with "Operation not permitted". This means existence checks are not sufficient to determine readability.
- Granting FDA to a compiled binary is fragile: each recompilation changes the binary's code signature, invalidating the grant. Granting FDA to the terminal app is more practical for development.
- Any future Safari history parser should be treated as experimental until it is validated against real local Safari data.
