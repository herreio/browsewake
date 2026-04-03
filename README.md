# Browsewake

A CLI tool that exports browser tabs and per-tab navigation history from Firefox, Chrome, and Safari.

Browser extensions can export active tabs but not their back/forward history. Browsewake reads browser session files directly, providing two dimensions:

- **Synchronous** — all open tabs at a point in time
- **Diachronic** — each tab's full navigation trail (back/forward list)

## Browser support

| Browser | Current tabs | Per-tab history | Data source |
|---------|:---:|:---:|---|
| Firefox | Yes | Yes | `recovery.jsonlz4` (mozlz4-compressed JSON) |
| Chrome  | Yes | Yes | SNSS session files |
| Safari  | Yes | No  | JXA (AppleScript) / SQLite fallback |

Safari does not store per-tab navigation history in any accessible format.

## Installation

```
cargo install --path .
```

Or build from source:

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
```

## Platform support

Browsewake is cross-platform. The session file formats are identical across OSes — only paths differ.

| Browser | macOS | Linux | Windows |
|---------|---|---|---|
| Firefox | `~/Library/Application Support/Firefox/Profiles/*/` | `~/.mozilla/firefox/*/` | `%APPDATA%\Mozilla\Firefox\Profiles\*\` |
| Chrome  | `~/Library/Application Support/Google/Chrome/` | `~/.config/google-chrome/` | `%LOCALAPPDATA%\Google\Chrome\User Data\` |
| Safari  | `~/Library/Safari/` | N/A | N/A |

## License

MIT
