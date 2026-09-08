# Contributing

Use the Rust version in `rust-toolchain.toml` and Python 3.9+ for release checks.
Keep changes scoped; do not use real credentials in tests or screenshots.

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
python3 scripts/release.py check
python3 -m unittest discover -s scripts -p 'test_*.py'
```

Use `CODEX_ACCOUNT_HUB_PREVIEW=accounts cargo run --features visual-qa` for an inert
sample-data preview. Do not enable `visual-qa` in a release build.

Changes to storage or account activation need regression tests with temporary
directories and synthetic auth. UI changes must preserve confirmation steps and
be checked with long names, missing/stale quota, login, errors, and keyboard input.
Do not add automatic quota polling or account switching as incidental cleanup.

Do not submit `auth.json`, account snapshots, registry files, access tokens,
device codes, private logs, signing certificates, or personal screenshots.
See [SECURITY.md](SECURITY.md) for sensitive reports.
