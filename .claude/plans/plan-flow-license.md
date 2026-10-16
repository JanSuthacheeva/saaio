# Plan: licensing

## Decision

flow is dual licensed MIT OR Apache-2.0, the Rust convention. The texts are the canonical ones:
LICENSE-APACHE is the Apache Software Foundation's LICENSE-2.0.txt unchanged (its appendix keeps
the `[yyyy]` placeholder, as it is instructions), LICENSE-MIT is the MIT License with
`Copyright (c) 2026 Jan Suthacheeva`. No other changes.

## Units

- `LICENSE-MIT`, `LICENSE-APACHE` (new, repo root).
- `Cargo.toml`: `license = "MIT OR Apache-2.0"` in `[package]`.
- `README.md`: a `## License` section at the end, naming both licenses with links to the files
  and the usual contribution clause (contributions are dual licensed as above, without
  additional terms or conditions).

## Data

None. No code, flag or output changes.

## Tasks

- [x] LICENSE-MIT and LICENSE-APACHE at the repo root, `license` in Cargo.toml, a License section
      in README.md; `cargo build` reads the manifest without warnings

## Review

- **Fixed** - nothing; no findings.
- **Verified** - LICENSE-APACHE is byte-identical to apache.org's LICENSE-2.0.txt; LICENSE-MIT is
  the standard text with `Copyright (c) 2026 Jan Suthacheeva`; `cargo metadata` reports
  `license = "MIT OR Apache-2.0"`; no other file in the repo carries a copyright or license
  notice; every dependency in Cargo.lock is under a permissive license compatible with
  MIT OR Apache-2.0 (MIT, Apache-2.0, BSD-2, Zlib, Unicode, Unlicense, WTFPL, or an option of these).
- **Open** - none. Optional later: a `cargo deny` license check in CI before the public release,
  so a copyleft dependency cannot slip in unnoticed.
- **Where to look** - `README.md` `## License` (the wording visitors read) and `LICENSE-MIT`
  (the copyright line). LICENSE-APACHE and Cargo.toml are routine and can be skipped.
- **Checks** - tests, clippy and fmt passed at 870f04f; nothing but the plan changed since.
