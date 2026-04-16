# appclean

A macOS command-line app cleaner. When you drag an app to the Trash, macOS leaves behind preference files, caches, support data, and more scattered across `~/Library`. `appclean` finds and removes all of it.

## Features

- Scans all standard macOS library locations for associated files
- Interactive multi-select — choose exactly what to remove
- **Moves files to a recoverable trash by default** — restore at any time with `appclean restore`
- `--permanent` to skip the trash and delete immediately
- `--dry-run` to preview what would be removed without touching anything
- Progress bar during move/delete
- Matches by both bundle ID (`com.tinyspeck.slackmacgap`) and app name (`Slack`)

## Installation

### From source

Requires [Rust](https://rustup.rs) 1.70 or later.

```sh
git clone https://github.com/tdelam/appclean
cd appclean
cargo install --path .
```

## Usage

### Remove an app (recoverable — default)

Files are moved to `~/.appclean/trash/` rather than permanently deleted.

```sh
appclean /Applications/Slack.app
```

### Restore a previous removal

Lists all past sessions and lets you pick one to restore.

```sh
appclean restore
```

### Remove an app permanently

Skips the trash and deletes immediately. **This cannot be undone.**

```sh
appclean --permanent /Applications/Slack.app
```

### Preview what would be removed

Shows everything that would be removed without touching anything.

```sh
appclean --dry-run /Applications/Slack.app
```

### Skip the confirmation prompt

```sh
appclean --yes /Applications/Slack.app
```

### Empty the trash

Permanently remove all sessions from the appclean trash (frees disk space):

```sh
appclean empty-trash
```

Only remove sessions older than 30 days:

```sh
appclean empty-trash --older-than 30
```

## Trash location

By default, removed files are moved to:

```
~/.appclean/trash/<timestamp>-<AppName>/
```

Each session includes a `manifest.json` that records the original file paths, which is what `appclean restore` uses to put everything back.

To permanently clear all sessions at once:

```sh
appclean empty-trash
```

## Locations scanned

| Location | Purpose |
|---|---|
| `~/Library/Application Support/<name>` | App data |
| `~/Library/Caches/<bundle-id>` | Cached data |
| `~/Library/Preferences/<bundle-id>.plist` | Preferences |
| `~/Library/Logs/<name>` | Log files |
| `~/Library/Containers/<bundle-id>` | Sandboxed app container |
| `~/Library/Group Containers/<bundle-id>` | Shared container |
| `~/Library/Saved Application State/<name>.savedState` | Window state |
| `/Library/Application Support/<name>` | System-level app data |
| `/Library/Caches/<bundle-id>` | System-level cache |
| `/Library/Preferences/<bundle-id>.plist` | System-level preferences |

> **Note:** Deleting files under `/Library` may require running with `sudo`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).
