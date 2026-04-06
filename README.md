# Browsewake

A CLI tool that exports browser tabs and per-tab navigation history from Firefox, Chrome, Brave, and Safari.

Browser extensions can export active tabs but not their back/forward history. Browsewake reads browser session files directly, providing two dimensions:

- **Synchronous** — all open tabs at a point in time
- **Diachronic** — each tab's full navigation trail (back/forward list)

## Browser support

| Browser | Current tabs | Per-tab history | Deep history | Data source |
|---------|:---:|:---:|:---:|---|
| Firefox | Yes | Yes | — | `recovery.jsonlz4` (mozlz4-compressed JSON) |
| Chrome  | Yes | Yes | Yes | SNSS session files + History SQLite DB |
| Brave   | Yes | Yes | Yes | SNSS session files + History SQLite DB |
| Safari  | Yes | No  | — | JXA (AppleScript) |

**Deep history** (`--deep-history`): Chromium browsers record visits in a SQLite database with per-tab attribution. Browsewake anchors those visits to the current SNSS session history and reconstructs a causally connected visit tree for each tab. This is supplemental visit history, not an exact dump of the browser's visible back/forward list, and it is not used to extend `history[]`.

Safari remains current-tabs-only. Browsewake does not currently expose Safari per-tab back/forward history because no stable standalone CLI surface has been validated for it.

Parser details, output semantics, and upstream references are documented in [SOURCES.md](SOURCES.md).

## Installation

### Prebuilt binaries

Download the latest release for your platform from [GitHub Releases](https://github.com/herreio/browsewake/releases). Binaries are available for:

- macOS (Apple Silicon, Intel)
- Linux (x86_64, aarch64)
- Windows (x86_64)

On macOS, downloaded binaries are blocked by Gatekeeper. Remove the quarantine attribute after extracting:

```
xattr -d com.apple.quarantine /path/to/browsewake
```

### From source

```
cargo install --path .
```

Or build manually:

```
cargo build --release
```

## Usage

```
browsewake [OPTIONS] [BROWSER...]
```

Export all detected browsers:

```
browsewake
```

Export a specific browser:

```
browsewake firefox
```

When a browser is explicitly requested but unsupported on the current platform or not installed, browsewake exits with an error instead of silently returning an empty export.

### Options

```
-f, --format <FORMAT>  Output format: json, text, csv [default: json]
    --no-history       Skip per-tab navigation history
    --deep-history     Augment Chromium tabs with anchored visit history from the History database
    --compact          Compact JSON (no pretty-printing)
-o, --output <FILE>    Write to file instead of stdout
```

### Examples

```sh
# Pretty JSON to stdout (default)
browsewake

# Compact JSON for piping
browsewake --compact | jq '[.browsers[].windows[].tabs[]] | length'

# Plain text overview
browsewake --format text

# CSV export to file
browsewake --format csv -o tabs.csv

# Only current URLs, no history
browsewake --no-history

# Single browser
browsewake firefox --format text

# Anchored Chromium visit history (supplemental, not CSV-exported)
browsewake chrome --deep-history
```

## Platform support

Browsewake is cross-platform. The session file formats are identical across OSes — only paths differ.

| Browser | macOS | Linux | Windows |
|---------|---|---|---|
| Firefox | `~/Library/Application Support/Firefox/Profiles/*/` | `~/.mozilla/firefox/*/` | `%APPDATA%\Mozilla\Firefox\Profiles\*\` |
| Chrome  | `~/Library/Application Support/Google/Chrome/` | `~/.config/google-chrome/` | `%LOCALAPPDATA%\Google\Chrome\User Data\` |
| Brave   | `~/Library/Application Support/BraveSoftware/Brave-Browser/` | `~/.config/BraveSoftware/Brave-Browser/` | `%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\` |
| Safari  | `~/Library/Safari/` | N/A | N/A |

## License

MIT
