# Contributing

## Set up once per clone

```
git config core.hooksPath .githooks
cp scripts/sensitive-patterns.example .sensitive-patterns   # then add your own words
```

The hooks:

- **pre-commit** runs `scripts/check_sensitive.py --staged` and blocks commits holding
  secrets or personal data (home paths, host names, LAN IPs, e-mail addresses, keys).
  Mark a real false positive with a `sensitive-scan: allow` comment.
- **post-commit** records the commit's times in UTC.
- **pre-push** refuses commits whose times aren't in UTC.

Scan the whole history with `python scripts/check_sensitive.py --history`.

## Commit times

Commit times carry the committer's time zone, so every commit here is stamped in UTC
(+0000). If a branch has non-UTC commits, fix it with `sh scripts/checks/utc_fix.sh`.

## Rules

- No secrets, personal data or real screenshots in code, tests, docs or commits.
- No network calls. Onion Peel has none, and SECURITY.md says so.
- Anything that changes what you see: post before and after pictures in the PR.

## Release builds

Build anything you'll share with `powershell -File scripts/build-release.ps1`, not plain
`cargo build`. Rust keeps source paths in the exe, and the paths of downloaded crates
contain your user name. The script maps them to neutral names, then checks the exe for
your user name and home folder, and prints its SHA-256.

## Self-test

```
cargo build --release --features selftest
target/release/onion-peel.exe --selftest <folder>
```

It times a capture, checks the clipboard packing, and renders the snip screen and every
install page (dark and light, 100% and 150%) to BMP files, from a fixed test pattern, so
two builds can be compared byte for byte. `capture.bmp` is a real screenshot: delete it.
