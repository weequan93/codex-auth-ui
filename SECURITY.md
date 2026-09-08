# Reporting sensitive issues

This utility handles local authentication files. Never attach credentials,
account registries, device codes, or unredacted logs to a public issue.

If the repository's **Security → Report a vulnerability** option is available,
use it for a private report. Otherwise open a minimal issue asking the maintainer
for a private reporting channel, without exploit details or sensitive data.

Include the app version, OS version, expected behavior, and a synthetic reproduction.
Only the latest release is intended to receive fixes; no response-time guarantee
or independent security audit is claimed.

Local account snapshots are permission-restricted files, not encrypted storage.
This app cannot coordinate writes atomically with every other Codex client. Quit
active sessions before switching and run only one Hub instance. Release artifacts
must never include user account data or signing secrets.
