# Build and installation guide

The maintained public guides are now:

- [Install, upgrade, uninstall, and troubleshoot](install.md)
- [Versioning, signing, and GitHub draft releases](releasing.md)

## One-command local app build

From the project directory, with rustup, Xcode command-line tools, and Python 3.9+:

```sh
bash scripts/package-macos.sh
```

Add `--dmg` to create a drag-to-Applications disk image with checksums and
third-party notices. The version and build number come from Cargo.toml;
there is no separate launcher or duplicated manual version recipe.

The script creates a fresh directory under `dist/` and does not install or launch
the app. Default builds are ad-hoc signed, not Developer ID signed or notarized.
Read the release guide before distributing a public binary.

Account credentials and app backups must never be included in a source release.
Use `python3 scripts/release.py export` to prepare an allowlisted source snapshot.
