# Maintainer release guide

## Repository setup

Use a dedicated repository for this app. **Check `git rev-parse --show-toplevel`
before any add, commit, or push.** Do not publish a parent workspace with unrelated
projects, private logs, or old installed-app backups.

```sh
python3 scripts/release.py check
python3 scripts/release.py export
```

The export prints a fresh, allowlisted source directory and ZIP under `dist/`.
It includes reviewed source, public docs, and workflows; excludes account data,
local reports, build outputs, and unrelated files. Inspect the result before
initializing a dedicated repository there. It is not a complete secret scanner.
Do not use `git add .` in an unverified parent repository.

The dedicated repository is [weequan93/codex-auth-ui](https://github.com/weequan93/codex-auth-ui).
Configure branch protection, required CI, and private vulnerability reporting on
GitHub. None of these remote settings is changed by the local tooling.

## Versioning

`Cargo.toml` is the source of truth:

- `package.version`: `MAJOR.MINOR.PATCH`; tag it as `vMAJOR.MINOR.PATCH`.
- `package.metadata.release.build-number`: increment for every distributed build,
  including a rebuild with the same user-facing version. Do not replace a published
  tag or artifact; normally use a new patch version for a corrected public build.
- Update the root package entry in `Cargo.lock` and add a matching `CHANGELOG.md`
  section. Record the release date when publishing; unreleased work stays Unreleased.
- The UI, User-Agent, app version, and artifact filenames derive from Cargo metadata.
  The package script derives the bundle build number from the same file.

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
python3 scripts/release.py check --tag v0.2.1
python3 -m unittest discover -s scripts -p 'test_*.py'
bash scripts/package-macos.sh --dmg
```

Change the example tag to the intended version. Source changes require a rebuild.

## GitHub workflow

1. Review and commit a clean release in the dedicated repository. Wait for CI.
2. Create and push the matching tag only when ready. Pushing a `vMAJOR.MINOR.PATCH`
   tag automatically starts the release workflow. It refuses a version mismatch.
3. Alternatively, run **Actions → Prepare draft macOS release → Run workflow**,
   entering an existing tag. Use this for a failed build retry; do not run both
   methods for an already successful tag.
4. The workflow tests and builds Apple Silicon and Intel DMGs, verifies downloaded
   checksums, attaches dependency inventories, and creates a **draft prerelease**.
5. Review the draft, license obligations, and platform tests before clicking Publish.
   There is no automatic public publishing. A draft with the same tag is not silently
   overwritten; inspect an existing draft before rerunning.

For the current version, after committing all release changes and seeing green CI:

```sh
git push -u origin main
git tag -a v0.2.1 -m "Codex Account Hub v0.2.1"
git push origin v0.2.1
```

Change the version for future releases. Never force-push or reuse a published tag.
Pushing a branch runs checks on macOS and Windows; pushing a version tag runs the
macOS release builds. Apple Silicon uses `macos-15`; Intel uses `macos-15-intel`.
Actions must be enabled in repository settings. Workflow builds use hosted runner
minutes (billing depends on the repository visibility and account plan).

The build jobs have read-only repository permissions. Only the final draft job has
contents-write permission. Actions are pinned to immutable commits and Dependabot
proposes updates. No personal token or Apple signing secret is required for the
default draft workflow. Never run unreviewed workflow changes with signing secrets.

## Signing and public launch gate

Default builds have an ad-hoc local signature. **They are not notarized and are not
equivalent to a trusted, frictionless public installer.** The workflow labels this
explicitly. Before a general public release, choose whether to provide an honestly
labeled experimental build or complete Developer ID signing/notarization.

For notarization: use a publisher-owned bundle identifier and Developer ID Application
identity, sign the executable and bundle with hardened runtime and a timestamp,
submit with `notarytool`, require Accepted, staple and validate the app, then recreate,
sign, notarize and staple the DMG. Regenerate checksums after final signing. Do not
commit certificates, private keys, passwords, or App Store Connect keys.

Apple's [Developer ID documentation](https://developer.apple.com/developer-id/) is
the reference for distribution signing; this repository does not currently automate
that credential-bearing workflow.

## Before publishing

- [ ] Correct repository, version, build number, immutable tag, and dated changelog.
- [ ] CI is green on GitHub (local checks alone do not establish that).
- [ ] No credentials, personal screenshots, private paths, or backups in source/assets.
- [ ] MIT authorship and dependency licensing reviewed. The generated inventory
   lists declared licenses; bundled dependency texts are collected in
   THIRD_PARTY_NOTICES.txt. Resolve every missing-text warning and review other
   applicable notice obligations before distribution.
- [ ] Downloaded/quarantined artifact launches on each advertised OS/architecture.
- [ ] Tray repeated clicks, Escape, search, filtering, and installation/upgrade tested.
- [ ] Real sign-in/switch/restart tested only with designated test accounts and consent.
- [ ] Signing/notarization status and known limitations described accurately.
- [ ] Final checksums match the exact uploaded artifacts.
- [ ] A human reviews and publishes the draft.

References: [GitHub runner architectures](https://docs.github.com/en/actions/reference/runners/github-hosted-runners),
[draft release CLI](https://cli.github.com/manual/gh_release_create), and
[workflow security guidance](https://docs.github.com/en/actions/reference/security/secure-use).
