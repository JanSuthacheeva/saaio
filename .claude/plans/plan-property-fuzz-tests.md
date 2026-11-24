# Plan: property and fuzz tests for the parsers

Review page: https://claude.ai/artifact/H6Q8EzeCQiZbZ9LLsCwfCk (approaches: https://claude.ai/artifact/WEk41hZDBk9cL7eedAYXxK)

Rust quality audit item 8 (https://claude.ai/artifact/SmextdM2HEKjvHEwSJv4gu).

## Decision

- Each parser gets one oracle in `src/fuzzing.rs`: a function that takes any input, runs the parser and panics when a property breaks. `cargo test` drives the oracles with proptest strategies (`src/strategies.rs`); five cargo-fuzz targets in `.fuzz/` drive them with raw bytes.
- The fuzz package lives in `.fuzz/`, not `fuzz/`: a second package with binaries makes `cargo install --git` fail ("multiple packages with binaries found", reproduced; `required-features` does not help); cargo skips dot folders when it searches a git repo. cargo-fuzz runs with `--fuzz-dir .fuzz`.
- `fuzzing` is the one public module besides `cli`, and only with the `fuzzing` feature, which installs never enable.
- Fix in this PR: `Store::append` after a torn line (a crash cut a write short) glues the next event onto it and both are lost. Append now starts a fresh line.
- PRs run the properties (in `cargo test`) and fmt and clippy over `.fuzz/`. A weekly `fuzz.yml` on nightly runs each target 5 minutes, not required. Why: a fuzzer finds new inputs over long runs, so a short run per PR finds little and makes a required check slow and nondeterministic; the properties are the deterministic gate.

## Units

### `src/fuzzing.rs` (new, top-level)

`#[cfg(feature = "fuzzing")] pub mod fuzzing;` and `#[cfg(all(test, not(feature = "fuzzing")))] mod fuzzing;` in `lib.rs`. Uses `proto`, `event`, `fold`, `adapter`, `workflow`; no test fixtures.

```rust
/// Bytes as a stream of frames, read as ClientMsg and again as ServerMsg: each read ends as its
/// header says (End, Cut, TooLarge(len)) or as the body decodes (a message that survives its
/// frame, or Decode, which ends the stream); never Io or Encode.
pub fn frames(bytes: &[u8]);

/// Any text as a log line: an event it reads as writes one line that reads back as the same
/// event, at EVENT_VERSION.
pub fn log_line(text: &str);

/// Any text as a whole log: every event it reads keeps log_line's property, and the events fold.
pub fn log(text: &str);

/// Any text as a Claude Code hook payload: what Adapter::ClaudeCode makes of it survives the log.
pub fn claude_code_hook(text: &str);

/// Any text as a Codex hook payload, for either approver.
pub fn codex_hook(text: &str);

/// What the header in front of `rest` says the next read must end as.
enum Header { End, Cut, TooLarge(usize), Body(usize) }
fn header(rest: &[u8]) -> Header;

/// One message type read over the whole stream, each read checked against its header.
fn read_all<T: Serialize + DeserializeOwned + PartialEq + Debug>(bytes: &[u8]);

/// Every event a payload means, through the adapter's whole surface.
fn hook(adapter: Adapter, text: &str, approver: Approver);
```

### `src/strategies.rs` (new, `#[cfg(test)] mod strategies;` beside `fixtures`)

```rust
/// Any text, control characters, quotes, newlines and non-ASCII included, up to 60 chars.
pub fn text() -> impl Strategy<Value = String>;
pub fn phase() -> impl Strategy<Value = Phase>;          // valid names
pub fn workflow() -> impl Strategy<Value = Workflow>;    // valid, 1 to 4 phases
pub fn flow_id() -> impl Strategy<Value = FlowId>;
pub fn session_id() -> impl Strategy<Value = SessionId>;
/// Every EventKind variant, fields at any value.
pub fn event_kind() -> impl Strategy<Value = EventKind>;
/// Event::new(at, kind): an event as saaio writes it, texts cleaned.
pub fn event() -> impl Strategy<Value = Event>;
/// Every ClientMsg and ServerMsg variant; Board is up to three flows folded from a Started event
/// and up to eight event_kind()s.
pub fn client_msg() -> impl Strategy<Value = ClientMsg>;
pub fn server_msg() -> impl Strategy<Value = ServerMsg>;
/// A recorded payload from src/adapter/fixtures/<harness> with up to three of its fields
/// replaced by any JSON value, nested ones included.
pub fn hook_payload(harness: &str) -> impl Strategy<Value = String>;
/// Any JSON value, up to depth 3.
pub fn json() -> impl Strategy<Value = Value>;
/// The variant's position by an exhaustive match (no wildcard): a new variant fails to compile
/// here until it has a strategy.
pub fn variant(kind: &EventKind) -> usize;
pub const VARIANTS: usize;
```

### `src/event.rs` (modified)

```rust
impl Event {
    /// Every readable event of a log's text, in order: from_line of each line.
    pub fn from_log(text: &str) -> Vec<Event>;
}
```

Tests move to `src/event/tests.rs` (past 400 lines).

### `src/store.rs` (modified)

```rust
pub fn append(&self, id: FlowId, event: &Event) -> Result<()>;   // opens read + append; "\n" first when torn
pub fn events(&self, id: FlowId) -> Result<Vec<Event>>;          // calls Event::from_log
/// The log ends inside a line: its last byte is not a newline. False for an empty log.
fn torn(log: &File) -> Result<bool>;
```

### `src/proto.rs` (tests only)

Tests move to `src/proto/tests.rs` (430 lines). `random_bytes_decode_or_fail_but_never_panic` is replaced by the `frames` properties. `fixtures::Noise` stays for `server/tests.rs`.

### `Cargo.toml`, `src/lib.rs` (modified)

```toml
[features]
fuzzing = []      # the oracles in saaio::fuzzing, for .fuzz/
[dev-dependencies]
proptest = "1"
```

### `.fuzz/` (new package `saaio-fuzz`)

Own `[workspace] members = ["."]`, `publish = false`, `[package.metadata] cargo-fuzz = true`, deps `libfuzzer-sys = "0.4"` and `saaio = { path = "..", features = ["fuzzing"] }`. `Cargo.lock` committed; `.fuzz/.gitignore`: `target`, `corpus`, `artifacts`, `coverage`. Each target is one line, as ripgrep's `fuzz/fuzz_targets/fuzz_glob.rs`:

```rust
#![no_main]
libfuzzer_sys::fuzz_target!(|bytes: &[u8]| saaio::fuzzing::frames(bytes));
```

| Target | Input | Oracle |
|---|---|---|
| `frames` | `&[u8]` | `fuzzing::frames` |
| `log_line` | `&str` | `fuzzing::log_line` |
| `log` | `&str` | `fuzzing::log` |
| `hook_claude_code` | `&str` | `fuzzing::claude_code_hook` |
| `hook_codex` | `&str` | `fuzzing::codex_hook` |

### CI and docs

- `ci.yml`: fmt job adds `cargo fmt --check --manifest-path .fuzz/Cargo.toml`; clippy job adds `cargo clippy --manifest-path .fuzz/Cargo.toml --locked -- -D warnings`.
- `.github/workflows/fuzz.yml` (new): `schedule` Mondays 07:00 and `workflow_dispatch`; ubuntu-24.04; nightly; cargo-fuzz installed pinned with `--locked`; matrix over the five targets; hook targets seeded from `src/adapter/fixtures/<harness>/` into `.fuzz/corpus/<target>/`; `cargo +nightly fuzz run --fuzz-dir .fuzz <target> -- -max_total_time=300`; on failure upload `.fuzz/artifacts/<target>` with `actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2`. `permissions: contents: read`. Not a required check.
- CONTRIBUTING.md: a Fuzzing section (run a target, replay a crash, turn the input into an example test) and the fuzz workflow under CI. CLAUDE.md: the build command, `fuzzing.rs`, `strategies.rs`, `.fuzz/`, this plan in the plan list. CHANGELOG `Fixed`: a log line cut short by a crash no longer takes the next event with it.

## Call chains

### `fuzzing::frames(bytes)`

1. driver -> `fuzzing::frames(bytes)`
2. `frames` -> `read_all::<ClientMsg>(bytes)`, then `read_all::<ServerMsg>(bytes)`
3. `read_all` -> `header(rest): Header`
4. `read_all` -> `proto::read_msg(&mut cursor): Result<Option<T>, Error>`
5. `Body(len)` + `Ok(Some(msg))` -> `proto::encode(&msg)` -> `read_msg`: `Some(msg)` again; cursor at `4 + len`; loop
6. `End` -> `Ok(None)`; `Cut` -> `Err(Cut)`; `TooLarge(n)` -> `Err(TooLarge(n))`; `Body` -> `Err(Decode)`: stop
7. Error path: any other pairing (Io, Encode, another length, a message that changes on re-encode) -> `panic!` naming header and result; the fuzzer saves the input, proptest shrinks it

### `fuzzing::log(text)`

1. driver -> `fuzzing::log(text)`
2. `log` -> `Event::from_log(text): Vec<Event>`
3. each event -> `log_line(&event.to_line())`: `Event::from_line(line) == Some(event)`, line has no `\n`
4. `log` -> `fold::fold(id, &events, None, now): Option<FlowView>` (None without Started is fine)
5. Error path: a panic in fold or a changed event -> found bug

### `fuzzing::codex_hook(text)` (claude_code_hook alike, one approver)

1. driver -> `codex_hook(text)` -> `hook(Adapter::Codex, text, Approver::User)`, `hook(.., Approver::Reviewer)`
2. `hook` -> `serde_json::from_str::<Value>(text)`; not JSON -> return (saaio hook logs and stops)
3. `hook` -> `adapter.turn`, `before_tool`, `command`, `asks` (must not panic)
4. `hook` -> `adapter.translate(&payload, &Workflow::builtin(), approver): Vec<EventKind>`
5. each kind -> `log_line(&Event::new(0, kind).to_line())`
6. Error path: a panic, or an event the log does not read back -> found bug

### `Store::append(id, event)`

1. caller -> `Store::append(id, &event): Result<()>`
2. `Store` -> `OpenOptions::new().read(true).append(true).open(log)`; NotFound -> `bail!("no flow {id}")`
3. `Store` -> `torn(&log): Result<bool>` (metadata len; `read_at` of the last byte)
4. `Store` -> `log.write_all(["\n" if torn] + event.to_line() + "\n")`, one call
5. Error path: read or write error -> Err "could not write flow {id}", as today

## Properties `cargo test` runs

| Test (module) | Generated | Checks |
|---|---|---|
| `any_event_survives_its_log_line` (event) | `event()` | `fuzzing::log_line(to_line)`; no newline in the line |
| `cleaning_an_event_twice_changes_nothing` (event) | `event()` | `Event::new(at, e.kind) == e` |
| `any_line_reads_back_as_it_was` (event) | `text()`; event lines cut or a byte changed | `fuzzing::log_line`, `fuzzing::log` |
| `the_strategy_makes_every_kind` (event) | 512 `event_kind()` | variants seen == `0..VARIANTS` |
| `any_message_survives_its_frame` (proto) | vecs of `client_msg()`, `server_msg()` | one stream, read back in order, then None |
| `any_bytes_read_as_their_header_says` (proto) | any bytes; valid streams cut anywhere or with a length or byte changed | `fuzzing::frames` |
| `a_log_cut_anywhere_keeps_every_whole_event_and_the_next` (store) | 1 to 6 `event()`s, a cut offset, one more event | `events()` == whole lines before the cut, then the new one |
| `any_payload_translates_into_events_the_log_keeps` (claude_code, codex tests) | `hook_payload(..)`, `text()` | `fuzzing::claude_code_hook`, `codex_hook` |

## Test seams

- `fuzzing`: example tests with one hand-made frame per Header case, and one proving the oracle panics on a wrong pairing (a reader that returns Io).
- `strategies`: `the_strategy_makes_every_kind`; messages covered by `ClientMsg::name`.
- `Store::append`: an example test (log ending in `{"v":1,"at":7,"ki`, then append) plus the property; real temp files.
- Fuzz targets: fmt and clippy on every PR; run weekly.

## Assumptions

1. proptest default 256 cases; `PROPTEST_CASES` raises it locally. Shrunk failures in `proptest-regressions/` are committed.
2. The frame property checks that a decoded message survives re-encoding, not byte equality: postcard ignores trailing body bytes and may accept non-minimal varints.
3. The Codex rollout line reader (`codex::approver`'s transcript) is out of scope.
4. Fuzzing runs on Linux only.
5. Five minutes per target, one weekly run.
6. A crash fails the weekly run and uploads the input; no issue is opened automatically.
7. No seed corpus committed; hook targets are seeded from the fixtures at run time.
8. Two appenders racing on a torn log may each add a newline; the empty line is skipped.

## Tasks

- [ ] Move `event.rs` tests to `src/event/tests.rs` and `proto.rs` tests to `src/proto/tests.rs`, unchanged
- [ ] Add `Event::from_log(text)` and make `Store::events` use it, with an example test
- [ ] Fix `Store::append` with `torn(log)`, test first: an example test of a torn log followed by an append
- [ ] Add proptest and `src/strategies.rs` (`text`, `phase`, `workflow`, `flow_id`, `session_id`, `event_kind`, `event`, `variant`, `VARIANTS`) with `the_strategy_makes_every_kind`
- [ ] Add the `fuzzing` feature and `src/fuzzing.rs` `log_line` and `log`, with `any_event_survives_its_log_line`, `cleaning_an_event_twice_changes_nothing`, `any_line_reads_back_as_it_was`
- [ ] Add `a_log_cut_anywhere_keeps_every_whole_event_and_the_next` in the store tests
- [ ] Add `client_msg`, `server_msg` strategies and `fuzzing::frames` (`Header`, `header`, `read_all`) with its example tests, `any_message_survives_its_frame`, `any_bytes_read_as_their_header_says`; drop the `Noise` proto test
- [ ] Add `json`, `hook_payload` strategies and `fuzzing::claude_code_hook`, `codex_hook` with `any_payload_translates_into_events_the_log_keeps` in both adapter test modules
- [ ] Add the `.fuzz/` package with the five targets and its lockfile; run each target locally on nightly for a minute
- [ ] CI: fmt and clippy over `.fuzz/` in `ci.yml`; new `fuzz.yml`; run it once with `workflow_dispatch` after push if possible
- [ ] Docs: CONTRIBUTING.md Fuzzing section and CI entry, CLAUDE.md (build command, structure, plan list), CHANGELOG Fixed line
- [ ] End to end: a scratch store with a log cut mid-line by hand, then `saaio report` appends and `saaio status --json` shows the new event's state; full `cargo test --all-features`, clippy, fmt, deny
