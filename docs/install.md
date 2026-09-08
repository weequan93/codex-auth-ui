# Install Account Hub

Account Hub is an independent, experimental utility, not an official OpenAI app.
It works with file-based Codex credentials, not OS-keychain-managed sign-ins.

## Downloaded macOS app

1. Open this repository's **Releases** page and read that release's limitations.
2. Choose `arm64` for Apple Silicon or `x86_64` for Intel. No Windows installer is
   provided yet. Intel runtime testing remains a release checklist item.
3. Verify the downloaded DMG against the accompanying `.sha256` file:

   ```sh
   shasum -a 256 -c Codex-Account-Hub-VERSION-ARCH.dmg.sha256
   ```

   Replace `VERSION` and `ARCH` with the actual downloaded filename components.
   A checksum checks file integrity; it is not proof of publisher identity.
4. Quit an existing Account Hub, open the DMG, and drag the app into Applications.
5. Eject the DMG and launch **Codex Account Hub** from Applications. The panel opens;
   subsequent tray clicks toggle it. There is no persistent Dock icon.

The default artifacts are **ad-hoc signed, not Developer ID signed or notarized**.
macOS may block downloaded builds. Review the source and provenance before using
the system's Privacy & Security approval flow. Never disable Gatekeeper globally
or run recursive quarantine-removal commands. Maintainers should notarize builds
before recommending them to nontechnical users.

The build deployment target is macOS 13, not a verified minimum-OS support claim.
Check each release's tested OS/architecture list before installing.

## Build locally (simplest trusted local route)

Install Rust through rustup and Apple's Xcode command-line tools, then from a clean
checkout run:

```sh
bash scripts/package-macos.sh
```

The pinned Rust toolchain is installed by rustup if needed. Python 3.9+ is used for
release checks. The script builds a fresh `.app`, validates its local signature,
and reveals it in Finder. It does not install, launch, or restart anything.
Add `--dmg` for a disk image. Build outputs and backups stay under `dist/`.

## First use

- Install the Codex CLI separately before adding an account. Hub's login child
  searches common Homebrew, per-user, and system executable directories on macOS.
- Use **Add account** for browser sign-in or **Device code** for the alternative flow.
- Search by name, email, or plan. **Selected** means credentials saved on disk,
  not the identity of an already-running client.
- Quota checks are manual and call an undocumented backend endpoint. Read the
  in-app disclosure; if this risk is unacceptable, do not use Refresh.
- Quit running sessions before switching. Open **… → Session & restart** for
  desktop guidance. Existing CLI sessions must be reopened manually.

## Troubleshooting

**No tray icon / app appears to quit:** open the installed `.app`, not an inner
executable or an old build. Run only one Hub copy. Check its version in the header.
On displays with limited menu-bar space, other status items may crowd the icon.

**CLI not found:** check `codex --version` in Terminal. Version-manager-only paths
may not exist in Finder's environment; a custom CLI-path preference is not yet
implemented.

**Different accounts in Hub and Codex:** confirm both use the same `CODEX_HOME`
and file-based auth. Hub does not update keychain sign-ins or verify live sessions.

**No quota/reset time:** a missing backend value is unknown, not zero. A passed
reset timestamp is a prompt to refresh, not proof of current available quota.

**Custom CODEX_HOME:** Finder may not inherit Terminal environment overrides.
Ensure the app and client resolve the same location before switching.

## Upgrade and uninstall

Quit Hub before replacing the app. Keep a backup of the previous `.app` if needed.
An upgrade should replace only the app bundle. To uninstall, quit and move the
app to Trash. **Do not delete `~/.codex`**: other clients share those credentials.
App preferences are separate from shared account data; removing the app does not
erase either automatically.
