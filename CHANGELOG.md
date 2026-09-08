# Changelog

Versions follow Semantic Versioning. Before 1.0, minor releases may change behavior;
patch releases focus on fixes and compatible refinements. Tags are `vMAJOR.MINOR.PATCH`.

## [0.2.1] - Unreleased

### Fixed

- Clear stale selected-account status when the shared auth file is removed or
  replaced with an account not saved in Hub. Malformed auth is reported, not rewritten.
- Clear errors for removed accounts from the Attention filter.
- Show the busy state immediately after accepting the quota disclosure.
- Avoid arithmetic overflow from extreme persisted quota timestamps/durations.
- Limit build-script file watching so Windows cross-checks do not traverse DMG
  staging links into Applications.

### Improved

- Keep All accounts in saved order when the selection changes. Add Go to selected
  to clear search/filters and scroll to that account without switching credentials.
- Display the app version in the header and separate account navigation code.
- Public installation, contribution, and release documentation.
- Pinned toolchain, CI checks, version/tag validation, clean source exports,
  and an opt-in draft GitHub release workflow with checksums.

## [0.2.0] - 2026-09-08

- Account search, keyboard focus shortcut, and Selected/Attention filters.
- Selected-first presentation without modifying saved account order.
- Compact side-by-side quota tiles and refreshed card styling.

## [0.1.0] - 2026-09-06

- Initial local macOS release: account management, manual quota checks,
  process-guarded switching, and confirmed desktop restart guidance.
- Follow-up build 3 fixes native tray toggling and Retina positioning.
