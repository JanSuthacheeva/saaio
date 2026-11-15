# Docs in the Laravel documentation style

Sample approved: `docs/repository.md` (rendered: https://claude.ai/artifact/T4HBtjaDdBKQ5LbFpoZM61).

## Decision

Rewrite the prose of the user docs in the voice and page structure of laravel.com/docs. Every fact,
setting, default, command, message, real output and link stays true to the code; this changes the
sentences and the order, not behaviour. Supersedes the two rules of `plan-flow-docs-voice.md` that
clash with it ("every page opens with something to run", "at most one colon-then-list per section");
its other rules stand (no em dash, no emoji, no first person, tables only for lookups).

Scope: `docs/*.md` except `HANDOVER.md` and `docs/design/`. `README.md` keeps its pitch and quick
start; only the prose of its docs-like sections changes, without an Introduction or section list.
`docs/README.md` stays the index, its "Covers" column updated where a page's sections changed.

## Page shape

```
# <Page title>                       sentence case, as today

- [Introduction](#introduction)       section list, nested one level for ### headings
- [<Section>](#<anchor>)
    - [<Subsection>](#<anchor>)

## Introduction                       what the page is about and why you would need it
## <Common case>                      sections from the common case to the advanced one
### <Subsection>
```

- Second person, friendly, why before how. A concept is introduced by a sentence ending in a colon,
  followed by its code block (command, config, real output).
- Callouts as GitHub alerts: `> [!NOTE]`, `> [!TIP]`, `> [!IMPORTANT]`, `> [!WARNING]`. Sparingly,
  at most a few per page.
- "Case | Result" tables become prose; tables stay for lookups: keys, flags, variables,
  placeholders, colors, exit codes, JSON fields, key bindings, hook events.
- Reference pages (`cli.md`, `json.md`, `files.md`) get the section list and Introduction; their
  flag, field and file tables stay.
- Every anchor another page, `examples/` or `src/` links to keeps resolving: an `##` heading named by
  a link is never renamed. Anchors in use:
  - cli.md: `--type`, `run-by-an-agent-or-a-script`, `saaio-remove`, `saaio-report`, `saaio-send`,
    `saaio-start`, `saaio-update`, `several-projects`, `stacked-flows`
  - configuration.md: `agents`, `errors`, `exclusive`, `file-location`, `kinds-of-flows`, `master`,
    `services`, `the-first-prompt`, `ui`, `use-your-own-workflow`, `without-matching-skills`
  - flows.md: `guided-and-auto`, `how-a-worker-reports`, `kinds`,
    `merge-requests-opened-or-merged-elsewhere`, `plain-sessions`, `research-flows`,
    `stacked-flows`, `states`, `the-master`
  - ui.md: `copying`, `keys`, `notifications`, `starting-a-flow`
  - install.md: `from-source`, `shell-completions`
  - files.md: `removing-saaio`, `the-shared-folder`, `the-store`
  - themes.md: `errors`; troubleshooting.md: `reporting-a-bug`, `the-terminal-ui`
- A fact a sentence restates is checked against the code before it is reworded; a fact found wrong
  (as `cat pint.log` in repository.md) is corrected to the code, and named in the commit.
- Illustrative snippets may be added where a concept has no example, only if they run as written
  against saaio.

## Assumptions

- No CHANGELOG entry for the prose; one `Fixed` line for doc facts found wrong, if any.
- `docs/README.md` table layout stays; only the "Covers" text changes.
- Code blocks of real output stay byte for byte.

## Tasks

- [x] Commit the plan, the plan's line in CLAUDE.md, and the sample `docs/repository.md` (with the `cat pint.log` correction)
- [x] Rewrite `docs/flows.md`
- [ ] Rewrite `docs/configuration.md`
- [ ] Rewrite `docs/ui.md`
- [ ] Rewrite `docs/install.md`
- [ ] Rewrite `docs/agents.md`
- [ ] Rewrite `docs/themes.md`
- [ ] Rewrite `docs/troubleshooting.md`
- [ ] Rewrite `docs/cli.md`: the bullets above the list become the Introduction, the commands a proper nested section list
- [ ] Rewrite `docs/json.md`
- [ ] Rewrite `docs/files.md`
- [ ] Update `docs/README.md` "Covers", and the prose of `README.md`'s docs-like sections
- [ ] CHANGELOG `Fixed` line for any doc fact corrected
- [ ] End to end: build the site (`cargo run --features site --bin saaio-site -- _site`), `cargo test --all-features`, read every rendered page and the GitHub rendering of two pages (alerts, nested lists)
