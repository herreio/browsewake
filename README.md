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
| Safari  | Yes | No  | — | JXA (AppleScript) / SQLite fallback |

Safari maintains back/forward history internally, but it is not available through stable scripting or documented on-disk formats.

**Deep history** (`--deep-history`): Chromium browsers record every page visit in a SQLite database with per-tab attribution. This recovers the complete navigation tree for each tab — including branches pruned from the back/forward stack — by walking the causal `from_visit` chain anchored to the current session data.

## Installation

### Prebuilt binaries

Download the latest release for your platform from [GitHub Releases](https://github.com/herreio/browsewake/releases). Binaries are available for:

- macOS (Apple Silicon, Intel)
- Linux (x86_64, aarch64)
- Windows (x86_64)

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

### Options

```
-f, --format <FORMAT>  Output format: json, text, csv [default: json]
    --no-history       Skip per-tab navigation history
    --deep-history     Augment Chromium tabs with full visit history from the History database
    --compact          Compact JSON (no pretty-printing)
-o, --output <FILE>    Write to file instead of stdout
```

### Examples

```sh
# Pretty JSON to stdout (default)
browsewake

# Compact JSON for piping
browsewake --compact | jq '.browsers[].tabs | length'

# Plain text overview
browsewake --format text

# CSV export to file
browsewake --format csv -o tabs.csv

# Only current URLs, no history
browsewake --no-history

# Single browser
browsewake firefox --format text

# Full visit history for Chrome/Brave tabs (from History DB)
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
