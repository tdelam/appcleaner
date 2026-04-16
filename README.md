# appclean

A macOS command-line app cleaner. When you drag an app to the Trash, macOS leaves behind preference files, caches, support data, and more scattered across `~/Library`. `appclean` finds and removes all of it.

## Features

- Scans all standard macOS library locations for associated files
- Interactive multi-select — choose exactly what to remove
- `--dry-run` to preview what would be deleted without touching anything
- Progress bar during deletion
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

```sh
# Interactive — select which files to delete, then confirm
appclean /Applications/Slack.app

# Preview what would be deleted without deleting anything
appclean --dry-run /Applications/Slack.app

# Skip the confirmation prompt
appclean --yes /Applications/Slack.app
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
