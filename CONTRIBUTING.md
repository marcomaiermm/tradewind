# Contributing

Bug reports and pull requests are welcome. If sync works or breaks for your account, an issue about that helps too.

## Setup

You need Rust 1.92 or newer, the OpenSSL headers, `pkg-config`, `bsdtar` and `zip`. Before opening a PR, run:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

The tests never touch your WoW folder or your TSM account.

## A few rules

- User-facing text goes into `src/i18n/catalog.rs`, in English and German.
- Writes into the WoW folder go through `adapters::files` and `adapters::archives`. Lua is parsed, never executed.
- Don't log passwords, tokens or raw server responses.
- Have a look at [docs/architecture.md](docs/architecture.md) before bigger changes.

## Commits

PRs get squash-merged, so the PR title becomes the commit message. Use [Conventional Commits](https://www.conventionalcommits.org) (`fix:`, `feat:`, `docs:` and so on). release-please builds the changelog and the releases from those titles.
