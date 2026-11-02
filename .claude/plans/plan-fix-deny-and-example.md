# Fix: cargo deny fails on RUSTSEC-2026-0414, and two broken sentences in the config example

Report: from the saaio master - "cargo deny fails on main and every PR: RUSTSEC-2026-0414 in
pulldown-cmark 0.13.4. Bump to >=0.13.5. In the same PR fix two text mistakes in
examples/config.toml (and the matching wording in docs/configuration.md and src/config): under
[agents] the 'prompt' description breaks off mid-sentence; '{id} is flow's id' is missing 'the'.
Add a CHANGELOG line under Unreleased for the dependency fix."

## Report

Expected: `cargo deny check` passes; the [agents] comments in the config example read as whole,
correct sentences.
Actual: the deny job fails on every PR; the example says "Unset, the flow's kind's" and
"{id} is flow's id for the session".
Steps and environment: CI deny job (`.github/workflows/ci.yml`), main at 95aec22.

## Reproduction

    cargo deny check advisories
    # ├ ID: RUSTSEC-2026-0414
    # ├ Solution: Upgrade to >=0.13.5 (try `cargo update -p pulldown-cmark`)
    # ├ pulldown-cmark v0.13.4
    #   └── saaio v0.2.0
    # advisories FAILED

    sed -n 112p examples/config.toml
    # # prompt    the first prompt for every kind, filled into {prompt}. Unset, the flow's kind's
    sed -n 118p examples/config.toml
    # # Placeholders: {id} is flow's id for the session, ...
    sed -n 306p docs/configuration.md
    # | `{id}` | flow's id for the session |

Fails every run.

## Root cause

- `Cargo.toml:23` requires `pulldown-cmark = "0.13.4"` and `Cargo.lock` pins 0.13.4, which the
  advisory database now flags (wikilinks with an empty display text blow up the event stream; fixed
  in 0.13.5). saaio is not exposed: `src/ui/markdown.rs:16` uses `Parser::new(text)`, which leaves
  `ENABLE_WIKILINKS` off, and no `Options` are set anywhere. The check fails all the same.
- `examples/config.toml:112` loses the noun at the end of the `prompt` line ("the flow's kind's"
  what?); `docs/configuration.md:300` says "Unset: the flow's kind's." the same way. Both came in
  with caa3f68 "config: kinds of flows". `examples/config.toml:118` and `docs/configuration.md:306`
  say "flow's id" without "the", unlike every other placeholder line ("the flow's name").
- `src/config.rs:32` ("None takes the flow's kind's") and `src/config.rs:340` ("both asked the
  flow's kind's prompt") are doc comments, not user-facing; line 340 is whole, line 32 is a terse
  but complete code comment. Neither mirrors the example text.

## Fix

- `Cargo.toml`: `pulldown-cmark = "0.13.5"`; `cargo update -p pulldown-cmark --precise 0.13.5`
  (or the latest 0.13.x) so `Cargo.lock` moves with it and nothing else.
- `examples/config.toml:112`: "Unset, the flow's kind's prompt applies" (wrapped to keep the file's
  100-column comments).
- `examples/config.toml:118`: "{id} is the flow's id for the session".
- `docs/configuration.md:300`: "Unset: the flow's kind's prompt."; `:306`: "the flow's id for the
  session".
- `src/config.rs:32`: "None takes the flow's kind's prompt." so the comment matches.
- `CHANGELOG.md` under `## [Unreleased]`, `### Fixed`: pulldown-cmark updated to 0.13.5 for
  RUSTSEC-2026-0414; saaio never enabled wikilinks, so it was not affected.

## Also affected

- `config::EXAMPLE` (`src/config.rs:329`) and `default_file::THEME_EXAMPLE` keep `earlier: &[]`.
  `DefaultFile::ensure` only rewrites a file that still holds the current text or one listed in
  `earlier`; anything else counts as edited. So an untouched config.toml written by 0.1.0
  (examples hash 9330ae82), 0.1.1/0.1.2 (b067ef56) or 0.2.0-rc.1 (820ca7de) is never refreshed:
  0.1.x users already keep an example without kinds, and this fix will not reach rc.1 users either.
  theme.toml is unchanged since 0.1.0, so it is not affected yet. Pre-existing, not caused by this
  report; see open question 1.
- No data written wrong.

## Assumptions

- No new unit test for the text: the example tests already uncomment `examples/config.toml` and
  compare it with the defaults, and the change is in comments only. The regression check for the
  dependency is `cargo deny check` itself.
- `src/config.rs:340` stays as is.

## Open questions

1. Register the released config.toml examples as earlier defaults (copies of the 0.1.0, 0.1.2 and
   0.2.0-rc.1 texts beside the example, listed in `config::EXAMPLE.earlier`, like master.md's
   `master-vN.md`), so an untouched example gets the fixed text on update? - recommended: yes, in
   this PR, since this PR changes the text and 0.1.x users are already stuck on an outdated
   example; it is additive and cheap to reverse.
   Decided (master, 2026-11-02): yes, in this PR, following the master-vN.md pattern, with its own
   CHANGELOG line.

## Tasks

- [x] Run `cargo deny check` (fails today with `advisories FAILED` on RUSTSEC-2026-0414), then bump
  pulldown-cmark to 0.13.5 in `Cargo.toml` and `Cargo.lock`; `cargo deny check`, `cargo test` and
  clippy pass
- [ ] Complete the `prompt` sentence and add "the" to `{id}` in `examples/config.toml`,
  `docs/configuration.md` and the `src/config.rs:32` comment
- [ ] Keep the released examples as `examples/config-v1.toml` (0.1.0), `config-v2.toml`
  (0.1.1/0.1.2) and `config-v3.toml` (0.2.0-rc.1), byte for byte from their tags, and list them in
  `config::EXAMPLE.earlier`; a test writes each into a scratch path and sees `ensure` replace it
  with the current example (fails today: the file is kept)
- [ ] Add the CHANGELOG lines under `## [Unreleased]`: the pulldown-cmark update, and an untouched
  config.toml example now updated to the current one
- [ ] End-to-end: `cargo deny check` prints `advisories ok`; a scratch `SAAIO_HOME` with a fresh
  `XDG_CONFIG_HOME` running `saaio server` once writes a config.toml whose [agents] lines read
  "Unset, the flow's kind's prompt applies" and "{id} is the flow's id"; then `saaio --stop`
