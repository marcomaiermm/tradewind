# Security policy

Please report vulnerabilities **privately** through [GitHub security advisories](https://github.com/marcomaiermm/tradewind/security/advisories/new) rather than public issues. You'll get a response within a few days.

In scope: anything that could leak TSM credentials or tokens, write outside the selected WoW folder, or execute code from addon archives, backups or server responses. Only the latest release is supported.

Known limitation: with **Use legacy HTTP** enabled, API traffic is unencrypted by design (see [docs/protocol.md](docs/protocol.md#transport)).
