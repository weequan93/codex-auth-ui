# Architecture and boundaries

| Module | Responsibility |
| --- | --- |
| `app.rs`, `app/navigation.rs` | Native UI, confirmations, local search and filters |
| `app/preview.rs` | Inert synthetic fixtures and UI regression tests |
| `worker.rs` | Serialized background commands, login lifecycle, quota requests |
| `storage.rs`, `auth.rs`, `model.rs` | File-based account registry, auth parsing and snapshots |
| `countdown.rs`, `usage.rs` | Cached reset display and manual backend parsing |
| `process_guard.rs`, `desktop.rs` | Guarded switching and confirmed cooperative restart |
| `macos_dock.m` | macOS accessory app, direct tray visibility and window positioning |
| `startup.rs`, `prefs.rs` | Packaged startup, login-child PATH, local preferences |

The UI sends commands to one worker. The worker reports results back for repaint;
quota requests are never scheduled automatically. Login uses a temporary private
CODEX_HOME and imports successful auth only after parsing it. Imported accounts
still pass through the activation guard.

macOS tray callbacks change native visibility immediately because a hidden egui
window may not repaint. A visibility-result event then updates UI state. Deferred
startup hides cannot override a newer show. The bundle points directly at the Rust
executable, with no launcher wrapper.

Registry writes use temporary-file replacement and retain a rolling backup.
Credential switching validates account identity and preserves refreshed credentials
when switching away. Optimistic change checks and rollback checks are best effort,
not a transaction shared with unrelated clients. Run one Hub and stop active Codex
sessions before changing credentials.

Known limits: no background sync, keychain support, automatic updater, launch-at-login,
cross-client transaction lock, or reliable live-session identity verification.
Process-name detection may not recognize every desktop helper. Windows compilation
does not establish native runtime or installer support.
