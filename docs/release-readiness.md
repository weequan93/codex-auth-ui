# Release readiness — 0.2.1, build 6

## Completed locally

- Separated account navigation from the main UI module.
- Kept saved account positions stable and added Go to selected scroll navigation.
- Fixed stale selected-account state after logout or external account replacement.
- Malformed auth is reported without changing the registry.
- Removed orphaned Attention errors when accounts are removed.
- Set busy state immediately after the quota disclosure is accepted.
- Prevented overflow from extreme quota timestamps and durations.
- Added the app version to the header; visually checked the synthetic native UI.
- Limited build-script watching and excluded build outputs from Cargo tracking.
- Replaced the long duplicate installation recipe with maintained public guides.
- Added MIT license, changelog, contribution/reporting guidance, pinned toolchain,
  CI, and an explicitly dispatched draft-only GitHub release workflow.
- Added version/tag validation, allowlisted source export, dependency inventories,
  packaged notices and per-DMG checksums.

Verification: 55 Rust tests, 4 release-tool tests, rustfmt, strict Clippy, Windows
GNU cross-target check, workflow YAML syntax, native sample screenshot, local bundle
signature/startup diagnostics and DMG integrity checks. GitHub CI passed on macOS
and Windows for commit `00741a6` (run `34254356554`). No real sign-in, quota refresh,
account switch, or desktop restart was used
for these checks. The installed app remains version 0.2.0.

## Public-release blockers / decisions

1. **Repository:** this app folder has its own Git repository, connected to
   `https://github.com/weequan93/codex-auth-ui.git`. Initial commit `00741a6` was
   pushed to main with user approval. Exactly 53 reviewed, allowlisted files were
   uploaded; local reports and builds were excluded. The unrelated parent repository
   is unchanged. GitHub CI passed. Tag `v0.2.1` has been pushed; release run
   `34254825642` passed both macOS architecture builds and created a draft prerelease.
   Its final job verified both installer checksums. The downloaded Apple Silicon
   installer also passed local SHA-256 and disk-image integrity checks. The draft
   has not been published. Node.js 20 action deprecation warnings remain; Dependabot
   has opened update proposals, which have not been merged without review.
2. **Signing:** builds are ad-hoc signed, not Developer ID signed or notarized.
   Choose explicitly labeled experimental distribution or complete publisher signing.
3. **Licensing:** upstream notices were retrieved at exact recorded commits and
   bundled. Two macOS-resolved dependencies lack complete packaged texts:
   `dispatch 0.2.0` and `hexf-parse 0.2.1`. Ten newer objc2-family packages contain
   upstream licensing discussions rather than complete texts. Review the generated
   DEPENDENCIES.md and resolve these obligations before public binary distribution.
   This inventory is not legal clearance or a complete independent license audit.
4. **Platform/runtime tests:** test the downloaded/quarantined artifact on intended
   macOS versions and Intel hardware. Windows is not a validated release platform.
   Real authentication workflows need designated test accounts and explicit consent.

## Known limitations retained

- One Hub instance at a time; no cross-client transaction lock or single-instance
  enforcement. Other clients can still race credential writes.
- Process-name detection is best effort and may miss desktop helpers.
- Selecting saved credentials does not verify or change an already-running session.
- Keychain-managed credentials, automatic updates, launch-at-login, and configurable
  version-manager CLI paths are not implemented.
- The manual quota endpoint is undocumented and may change or be unavailable.
- This work is a scoped bug/release audit, not a guarantee that no bugs remain or
  an independent security assessment.

Next: resolve signing/licensing/platform gates, review workflow-dependency updates,
then review and publish the draft manually.
