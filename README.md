# appclean

A macOS command-line app cleaner. When you drag an app to the Trash, macOS leaves behind preference files, caches, support data, and more scattered across `~/Library`. `appclean` finds and removes all of it — interactively, with a recoverable trash.

The binary is named **`apc`** for quick access (same idea as `rg` for ripgrep).

## Features

- Full terminal UI — navigate with keyboard, toggle files, confirm before deleting
- **Recoverable trash by default** — `apc restore` brings everything back
- `--permanent` to skip the trash and delete immediately
- `--dry-run` to preview what would be removed without touching anything
- Inline size bars and color-coded file types for fast visual scanning
- Matches by both bundle ID (`com.tinyspeck.slackmacgap`) and app name (`Slack`)
- Auto-purges trash sessions older than 30 days on every run

## Installation

Requires [Rust](https://rustup.rs) 1.70 or later.

```sh
git clone https://github.com/tdelam/appclean
cd appclean
cargo install --path .
```

This installs the `apc` binary into `~/.cargo/bin/`.

## Usage

### Remove an app

Files are moved to `~/.appclean/trash/` — restore them at any time.

```sh
apc /Applications/Slack.app
```

The interactive file selector opens in the terminal:

```
┌ Slack — 6/6 selected  (1.4 GB) ──────────────────────────────────────────────┐
│ ◉ /Applications/Slack.app                         ██████   286 MB  [app]     │
│ ◉ ~/Library/Application Support/Slack             ██████   960 MB            │
│ ◉ ~/Library/Caches/com.tinyspeck.slackmacgap      ███░░░   142 MB            │
│ ◉ ~/Library/Preferences/com.tinyspeck…plist       ░░░░░░     4 KB            │
└───────────────────────────────────────────────────────────────────────────────┘
  ↑↓/jk Navigate    Space Toggle    a Toggle all    Enter Confirm    q Quit
```

**Color key:** red = app bundle · yellow = cache · blue = preferences · magenta = containers · cyan = logs

**Keys:** `↑`/`↓` or `j`/`k` navigate · `Space` toggle · `a` toggle all · `Enter` confirm · `q` quit

### Restore a previous removal

```sh
apc restore
```

Lists all past sessions and lets you pick one to restore.

### Remove permanently (no trash)

```sh
apc --permanent /Applications/Slack.app
```

### Preview without deleting

```sh
apc --dry-run /Applications/Slack.app
```

### Skip the confirmation prompt

```sh
apc --yes /Applications/Slack.app
```

### Empty the trash

```sh
apc empty-trash                  # remove all sessions
apc empty-trash --older-than 30  # remove sessions older than 30 days
```

## Trash retention

Sessions are **automatically purged after 30 days** on every run. A one-line notice appears if anything was cleaned up. To empty manually:

```sh
apc empty-trash
```

## Trash location

Removed files are moved to:

```
~/.appclean/trash/<timestamp>-<AppName>/
```

Each session writes a `manifest.json` with original paths, which is what `apc restore` uses to put everything back.

## Locations scanned

| Location | Purpose |
|---|---|
| `~/Library/Application Support/<name>` | App data |
| `~/Library/Caches/<bundle-id>` | Cached data |
| `~/Library/Preferences/<bundle-id>.plist` | Preferences |
| `~/Library/Logs/<name>` | Log files |
| `~/Library/Containers/<bundle-id>` | Sandboxed app container |
| `~/Library/Group Containers/<bundle-id>` | Shared container |
| `~/Library/Cookies/<bundle-id>` | Cookies |
| `~/Library/Saved Application State/<name>.savedState` | Window state |
| `~/Library/WebKit/<bundle-id>` | WebKit storage |
| `~/Library/HTTPStorages/<bundle-id>` | HTTP caches |
| `/Library/Application Support/<name>` | System-level app data |
| `/Library/Caches/<bundle-id>` | System-level cache |
| `/Library/Preferences/<bundle-id>.plist` | System-level preferences |
| `/Library/Logs/<name>` | System-level logs |

> **Note:** Files under `/Library` (not `~/Library`) may require `sudo`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).
