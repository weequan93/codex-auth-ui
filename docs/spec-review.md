# Specification review and implementation plan

## Review outcome

The product direction is coherent: a small native tray app is a good match for the idle-power and startup goals, and manual-only quota refresh removes the always-on behavior that created the specification’s largest policy and suspension risk.

The supplied workspace contained no prior implementation, despite the specification referring to one. This project was therefore implemented from scratch.

The abbreviated data-model section was not sufficient by itself for safe interoperability. The implementation was checked against codex-auth’s current schema-v4 source and preserves its complete registry shape, including:

- `previous_active_account_key`
- `active_account_activated_at_ms`
- `interval_seconds`
- `account_name`
- `last_local_rollout`
- unknown future fields at registry and account level

## Key design decisions

1. A single serialized worker owns file mutations and the shared HTTP client. Normal UI interactions do not block on login, disk mutation, network activity, or process termination. Application shutdown waits for worker cleanup so an owned login process is not left running.
2. Every mutation reloads the registry first. Before replacement, the implementation checks that the source bytes did not change; a concurrent codex-auth write produces a retryable error instead of silently overwriting it.
3. Login runs in a private scratch `CODEX_HOME`, then imports the resulting auth snapshot without changing live credentials. Successful sign-in requests activation through the running-process guard. Cancellation, a ten-minute timeout, and shutdown reap the owned login process.
4. Refresh has no timer, startup request, or hidden retry. One acknowledged click produces at most one request for one account.
5. A cached window that has reset is not advanced or guessed. Its bar is replaced with “window reset — check again.”
6. Error badges remain in memory. The disclosure preference uses the app’s platform-standard config directory, completely outside `~/.codex`.

## Implemented sequence

1. Reproduce and preserve codex-auth’s schema-v4 data contract.
2. Implement auth parsing, safe snapshot naming, atomic registry/auth writes, switching, rename, removal, and login import.
3. Implement the shared manual quota client and response parser.
4. Add the tray, fixed popover, process guard, disclosure screen, account actions, cached quota display, and countdown.
5. Compile the full GUI shell and run the unit suite on macOS.
6. Build and visually inspect the native application against a synthetic `CODEX_HOME`.

## Completion audit

- Preserve refreshed live credentials when switching away, and use live credentials
  for manual quota checks when they match the requested account.
- Validate snapshot identity before activation. Roll back failed activation without
  overwriting an intervening writer's credentials detected during rollback.
- Keep shared legacy email snapshots when another workspace still references them.
- Replace Windows files without deleting the old destination first.
- Recheck running processes after an explicit stop request; do not activate if stopping fails.
- Preserve multiline device-code instructions, remove terminal formatting, and provide
  copy/cancel controls. Disable overlapping account actions during sign-in.
- Show the panel with an error if the tray cannot initialize.

The macOS regression suite contains 48 passing tests. macOS builds and Windows cross-target
checks pass. Native preview screenshots use synthetic accounts; no private quota
requests or real account-switching operations are part of these checks.

Local macOS packaging now launches the Rust binary directly, opens the installed
panel on startup, and adds Finder-safe CLI search paths only to login child processes.
The installed panel and bundle signature were checked; an isolated synthetic tray
preview no longer produced the previous repeated macOS status-bar activation errors.

## Deliberate limitations

- Process detection remains best-effort and matches only `codex`/`codex.exe`, as specified.
- Saved-credential selection is not live session synchronization. The UI labels
  saved credentials neutrally and explains the verification limit in a tooltip.
  Restart progress, accepted relaunch requests, and failures have distinct notices.
  macOS offers a separately confirmed cooperative
  restart of the exact desktop bundle; CLI restarts remain manual. Actual desktop
  quitting/reopening is not exercised in automated or visual tests.
- macOS tray clicks apply native visibility immediately and position the panel using
  AppKit screen points, clamped to the clicked display's usable area. They no longer
  depend on a hidden egui redraw or treat Retina pixels as window points. Pending
  startup hides cannot override a later show. Rapid queued visibility events and
  Escape/menu reopening have regression tests. On 8 September 2026, the user confirmed
  that the isolated native preview toggled on every click. Build 3 was then installed,
  its signature validated, and the real account panel visually verified. The old app
  was backed up and the synthetic test process closed. Escape-then-tray reopening
  was not separately confirmed by the user; that path has automated coverage only.
- Popover positioning uses the tray click point rather than a platform-specific tray-icon bounding rectangle.
- API-key records are displayed but cannot be switched or quota-refreshed in v1.
- Local macOS app packaging is implemented. Public distribution signing/notarization,
  launch-at-login, global hotkeys, and automated quota/switching remain out of scope.
