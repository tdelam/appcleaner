# Contributing

Thanks for your interest in contributing to appclean!

## Prerequisites

- [Rust](https://rustup.rs) 1.70 or later
- macOS (the project is macOS-only)

## Building locally

```sh
git clone https://github.com/tdelam/appclean
cd appclean
cargo build
```

## Running tests

```sh
cargo test
```

## Linting

All clippy warnings are treated as errors in CI. Run locally before pushing:

```sh
cargo clippy -- -D warnings
```

## Submitting a pull request

1. Fork the repository and create a branch from `main`.
2. Make your changes and ensure `cargo test` and `cargo clippy -- -D warnings` both pass.
3. Keep commits focused — one logical change per commit.
4. Open a pull request with a clear description of what you changed and why.

## Reporting bugs

Please use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md) when opening an issue. Include the app you were trying to clean and any error output.
