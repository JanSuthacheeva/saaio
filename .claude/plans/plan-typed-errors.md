# Typed errors

Implementation page: https://claude.ai/artifact/FhM9EqtBpZFvHZHtcL2PAR
(approaches: https://claude.ai/artifact/Ctn8WXd3hmXoKuGdeRWpka)

Items 1 and 2 of the rust-quality-audit (https://claude.ai/artifact/SmextdM2HEKjvHEwSJv4gu).

## Decision

- Refusals travel as data: `Failure` carries a `Refusal`, printed as a stable `refusal:` key
  (`uncommitted | unmerged | untrusted`) between `error` and `help`; the UI decodes it.
  `Refusal::of` and `Refusal::phrase` go.
- Typed errors only where code branches (approach A): `proto::Error` (thiserror, a direct
  dependency) for encoding, writing and reading messages; `client::Answer` for the server's reply
  to a hello. anyhow stays everywhere else, `connect`, `running` and `close_flow` included.
- Every message keeps its current words, so server.log lines and printed errors do not change.

## Units

### `src/refusal.rs` (modified)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Refusal {
    Uncommitted,
    Unmerged,
    Untrusted,
}
```

Removed: `Refusal::of(&str)`, `Refusal::phrase(self)`. The producers write the words in place.

### `src/cli/output.rs` - `Failure` (modified)

```rust
pub struct Failure {
    pub message: String,
    pub help: Vec<String>,
    pub code: u8,
    /// What saaio ui asks the person about instead of showing the error.
    pub refusal: Option<Refusal>,
}

impl Failure {
    /// Exit code 1, a refusal: printed with its `refusal` key.
    pub fn refused(refusal: Refusal, message: impl Into<String>, help: Vec<String>) -> Failure;
    /// error: ..., refusal: ... (only on a refusal), help[n]: ...
    pub fn to_toon(&self) -> String;
}
```

`Failure::error`, `Failure::usage` and the literal at `cli/commands/update.rs:35` set
`refusal: None`. In `to_toon`'s `ErrorDocument`, `refusal` is
`#[serde(skip_serializing_if = "Option::is_none")]`, declared between `error` and `help`.

### `src/cli/output/remove.rs`, `src/cli/output/trust.rs` (modified)

`remove::uncommitted`, `remove::unmerged`, `trust::untrusted` build through `Failure::refused`
with `Refusal::Uncommitted`, `Refusal::Unmerged`, `Refusal::Untrusted`; signatures unchanged.
`remove::unchecked` and `trust::not_a_person` stay `Failure::error`.

### `src/ui/app/effect.rs` (modified)

```rust
/// What a flow command printed: its first line when it worked; when not, the error and its
/// refusal from the whole document, else the first line without "error: ".
pub fn outcome(success: bool, stdout: &str) -> Result<String, JobError>;

/// The `error` and `refusal` of a printed error document; None when it is not one.
fn printed_error(stdout: &str) -> Option<JobError>; // replaces error_message(line)
```

`printed_error` decodes the whole stdout with serde_toon into a private
`struct Printed { error: String, refusal: Option<Refusal> }` (`help` ignored). `JobError` is
unchanged.

### `src/proto.rs` - `proto::Error` (new)

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("message of {0} bytes is over the limit")]
    TooLarge(usize),
    #[error("the stream ended inside a message")]
    Cut,
    #[error("could not encode a message")]
    Encode(#[source] postcard::Error),
    #[error("could not decode a message")]
    Decode(#[source] postcard::Error),
}

impl Error {
    /// The read's deadline passed (WouldBlock or TimedOut): the stream is fine, it was quiet.
    pub fn timed_out(&self) -> bool;
}

pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, Error>;
pub fn write_msg<W: Write, T: Serialize>(writer: &mut W, message: &T) -> Result<(), Error>;
pub fn read_msg<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<Option<T>, Error>;
fn read_full<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<bool, Error>;
```

Nothing else in proto.rs used anyhow, so its import goes. `server/outbox.rs` and `server/conn.rs` compile unchanged
(`?` into anyhow, or errors ignored).

### `src/client.rs` (modified)

```rust
/// What the server said to a hello.
enum Answer {
    Welcome(Binary),
    /// It speaks another protocol, and why.
    Refused(String),
}

/// Says hello on `stream`: Err only when the exchange itself failed.
fn hello(stream: &mut UnixStream, role: Role) -> Result<Answer>;

impl Client {
    pub fn send(&mut self, message: &ClientMsg) -> Result<(), proto::Error>;
    pub fn recv(&mut self) -> Result<Option<ServerMsg>, proto::Error>;
}
```

Removed: `fn timed_out(&anyhow::Error)`. `connect`, `running`, `close_flow` keep their
signatures.

### Elsewhere

- `Cargo.toml`: `thiserror = "2.0.21"` under `[dependencies]`.
- `docs/cli.md`: the `saaio remove` refusal example shows `refusal: unmerged`; "Output, errors and
  exit codes" documents the `refusal` key and its three values as stable.
- `CHANGELOG.md`: an Added line under Unreleased for the `refusal` key.

## Data

| Type | Fields | Crosses |
| --- | --- | --- |
| `Refusal` | `uncommitted`, `unmerged`, `untrusted` (lowercase serde) | CLI stdout to the UI and agents |
| `Failure` | + `refusal: Option<Refusal>` | commands to `main.rs` |
| error TOON | `error`, `refusal` (only on a refusal), `help[n]` | stdout, public contract |
| `proto::Error` | Io, TooLarge(usize), Cut, Encode, Decode | proto to client, outbox, conn |
| `Answer` | Welcome(Binary), Refused(String) | private to client.rs |

```
$ saaio remove shop-221
error: flow/shop-221 has 1 commit on no other branch
refusal: unmerged
help[2]: "Merge or push flow/shop-221, then run `saaio remove shop-221` again",Run `saaio remove shop-221 --delete-unmerged` to delete it with its commits
```

## Call chains

### A. The UI removes a flow with uncommitted work

1. `App::on_input -> Effect::Run(Job::Remove(removal))`
2. `ui::run_job -> saaio remove <name>` (child process)
3. `commands::remove -> output::remove::uncommitted(name, path): Failure`
4. `remove::uncommitted -> Failure::refused(Refusal::Uncommitted, message, help): Failure`
5. `main.rs -> Failure::to_toon(): String` (error, refusal, help), exit 1
6. `ui::run_job -> effect::outcome(false, stdout): Err(JobError)`
7. `outcome -> printed_error(stdout): Some(JobError { message, refusal: Some(Uncommitted) })`
8. `ui.rs:205` skips the log line (refusal is Some) `-> App::on_ran: View::Confirm`
9. Error path: stdout that is not an error document (crash, panic) `-> printed_error: None ->`
   first line without `error: `, refusal None `->` notice and server.log, as today.

Same chain for `remove::unmerged` (`Refusal::Unmerged`) and `trust::untrusted` from
`saaio start` (`Job::Start`, `Refusal::Untrusted`).

### B. saaio remove closes the pane and the server is slow

1. `retire -> client::close_flow(socket, id, timeout): Result<Closing>`
2. `close_flow -> Client::connect(socket, Role::Control): Result<Option<Client>>`
3. `close_flow -> Client::send(&ClientMsg::CloseFlow(id)): Result<(), proto::Error>`
4. `close_flow -> Client::recv(): Result<Option<ServerMsg>, proto::Error>`
5. `Err(error) if error.timed_out() -> Ok(Closing::TimedOut)`
6. Error path: any other `proto::Error -> Err(error.into())`, anyhow, message unchanged.

### C. A hello to a server of another protocol

1. `Client::connect -> hello(&mut stream, role): Result<Answer>`
2. `hello -> proto::write_msg(stream, &ClientMsg::Hello { .. })?` (proto::Error into anyhow)
3. `hello -> proto::read_msg(stream).context("the saaio server did not answer")?`
4. `Welcome -> Ok(Answer::Welcome(server))`; `Refused -> Ok(Answer::Refused(reason))`
5. `connect: Answer::Refused(reason) -> bail!("{reason}. {}", another_saaio(pid))`
6. `running: Answer::Welcome(server) -> Some(server)`, `Answer::Refused(_) -> None`
7. Error path: another message or a closed stream `-> bail!`, as today.

## Test seams

- `Refusal`: unit, serde names round trip.
- `Failure::to_toon`: unit, the refusal line sits between error and help; absent without one.
- `remove::uncommitted`/`unmerged`, `trust::untrusted`: unit, each carries its refusal (replaces
  the `Refusal::of` tests in remove.rs and trust.rs).
- `outcome`: unit (`ui/app/tests/panes.rs`, `dialogs.rs`): refusal read from the key; a refusal
  message holding none of the old phrases still gives its refusal; non-TOON stdout falls back to
  the first line.
- `proto::Error`: unit, over the limit, cut off, undecodable bytes each give their variant;
  `timed_out` true only for WouldBlock and TimedOut.
- `client`: the fake-server tests keep passing; `running` against a refusing fake server gives
  `server: None`.
- CLI (`tests/cli/`): a refused `saaio remove` prints `refusal: uncommitted`.

## Assumptions

- `remove::unchecked` stays a plain error with no refusal key: the UI shows it, it does not ask.
- The refusal messages keep their words; only where they come from changes.
- The UI falls back to the first line when stdout is not a TOON error document.
- `proto::Error` messages are the current anyhow strings word for word.
- `Client::send`/`recv` return `proto::Error`; callers using `?` into anyhow are unchanged.

## Tasks

- [x] Add `thiserror` and `proto::Error` with `timed_out`; `encode`, `write_msg`, `read_msg`,
      `read_full` return it; unit tests for TooLarge, Cut, Decode and `timed_out`
- [x] `Client::send`/`recv` return `proto::Error`; `close_flow` uses `error.timed_out()`; remove
      `client::timed_out`
- [x] `client::Answer` and `hello -> Result<Answer>`; `connect` and `running` match on it; test
      `running` against a refusing fake server
- [x] `Refusal` serde (lowercase) with a round-trip test; `Failure.refusal`, `Failure::refused`,
      `to_toon` prints the key; unit tests for the key's place and absence
- [ ] `remove::uncommitted`/`unmerged` and `trust::untrusted` through `Failure::refused`; their
      tests assert the refusal field
- [ ] `effect::outcome` reads the whole document through `printed_error`; remove `Refusal::of`,
      `Refusal::phrase` and `error_message`; UI tests for the key, a reworded message and the
      first-line fallback
- [ ] CLI test: a refused `saaio remove` prints `refusal: uncommitted`
- [ ] docs/cli.md (remove example, errors section), CHANGELOG Added line, CLAUDE.md plan list
- [ ] End to end: in a scratch store, `saaio remove` a flow with uncommitted work from the
      command line (sees `refusal: uncommitted`) and from saaio ui in a PTY (the discard dialog
      opens); end with `saaio --stop`
