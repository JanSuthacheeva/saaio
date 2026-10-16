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

- [ ] LICENSE-MIT and LICENSE-APACHE at the repo root, `license` in Cargo.toml, a License section
      in README.md; `cargo build` reads the manifest without warnings
