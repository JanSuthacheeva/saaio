# Plan: the master answers a worker's question dialog

Implementation page: https://claude.ai/artifact/2JUyqwrNR8gJk5jkZVxcHJ
Approaches page: https://claude.ai/artifact/CaZ7wcQWqrVmdvqnA9J99N

## Decision

Bug: a Claude Code worker asks with AskUserQuestion; `saaio status <name> --full` shows only the first
question's text, and `saaio send` says sent while Claude Code holds the message behind the open
dialog, so the flow stays waiting.

Approach A: the Waiting event keeps every question of the dialog and `saaio status` lists them with
their options. When the master sends a message to a flow waiting on that dialog, `saaio server`
presses Esc into its pane, so Claude Code closes the dialog ("User declined to answer questions"),
ends the turn and hands the held message over as the next prompt. No new command; Codex never
reports a dialog, so nothing changes for it.

Verified in the repro (Claude Code 2.1.303, scratch store): the worker's `saaio wait` marks the
message delivered and Claude Code holds it; Esc closes the dialog, the Stop hook runs, and the held
message arrives at once; the worker wrote the master's answers correctly.

Decided in review: permission prompts are out of this bug (ROADMAP, Planned soon); `send` gets no
separate field, only the changed help line.

## Units

### src/event.rs (changed)

```rust
Waiting {
    question: String,
    /// The questions of the dialog the worker shows (Claude Code's AskUserQuestion); empty for a
    /// question asked in words or a permission prompt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    questions: Vec<Question>,
},

/// One question of a dialog: its header, text, options and whether several may be picked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub header: String,
    pub question: String,
    pub options: Vec<Choice>,
    pub multi_select: bool,
}

/// An option of a question: what the dialog lists and the line below it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub label: String,
    pub description: String,
}
```

`EventKind::cut_texts` also cuts each header, question, label and description to `TEXT_LIMIT`.
Every `Waiting { question }` pattern and literal gains `..` or `questions: Vec::new()`.

### src/adapter/claude_code.rs (changed)

```rust
/// The questions of an AskUserQuestion input; a malformed one is left out.
fn questions(input: &Value) -> Vec<Question>;
```

`translate`: `"AskUserQuestion"` -> `Waiting { question: cut(questions[0].question), questions:
questions(input) }`; nothing when there is no first question, as today. `adapter/codex.rs` builds
`Waiting` with `questions: Vec::new()`.

### src/view.rs (changed)

```rust
pub struct FlowView {
    // ...
    /// The questions of the dialog the flow waits on; empty when it waits on none.
    pub questions: Vec<Question>,
}
```

New `--json` key `questions` (camelCase, `multiSelect`); `VIEW_VERSION` stays 3.

### src/fold.rs (changed)

`Folding::apply`: `Waiting { question, questions }` sets `view.questions`; after the match, a state
other than Waiting clears them. `shown` adds `&view.questions`.

### src/wake.rs (changed)

```rust
/// True when the master sent a message after `flow` opened the question dialog it still waits on.
pub fn dialog_answered(flow: &FlowView, events: &[Event]) -> bool;
```

False when `flow.questions` is empty; else true when a `Message` follows the log's last `Waiting`.

### src/server/dismiss.rs (new)

```rust
/// How long after an Esc the server may press it again into a dialog still open.
#[cfg(not(test))]
pub(super) const AGAIN: Duration = Duration::from_secs(30);
#[cfg(test)]
pub(super) const AGAIN: Duration = Duration::from_secs(3);

impl Server {
    /// Presses Esc into each running pane whose flow waits on a question dialog the master
    /// answered, once nobody typed there for QUIET, so Claude Code closes it and hands the held
    /// message over; again after AGAIN while it stays open.
    pub(super) fn dismiss_dialogs(&mut self);

    /// The flow behind a running worker pane whose dialog the master answered.
    fn answered_dialog(&self, key: &PaneKey) -> Option<&FlowView>;
}

/// True when Esc may go into a pane: open and untouched by the person for QUIET, no Esc within
/// AGAIN.
fn due(
    typed_at: Option<Instant>,
    opened_at: Option<Instant>,
    dismissed_at: Option<Instant>,
    now: Instant,
) -> bool;

const ESC: &[u8] = b"\x1b";
```

### src/server.rs (changed)

Field `dismissed_at: HashMap<PaneKey, Instant>`: when the server last pressed Esc into each pane;
an entry is dropped once its flow no longer waits on an answered dialog. `serve` calls
`self.dismiss_dialogs()` after `self.type_lines()`, both after `self.refresh()`. `mod dismiss;`.

### src/cli/output/status.rs (changed)

`detail` adds two tables while the flow waits on a dialog, texts cut to 80 chars unless `--full`:

```
questions[2]{n,header,question,pick}:
  1,Color,Which color?,one
  2,Toppings,Which toppings?,several
options[6]{question,n,label,description}:
  1,1,Red,"A warm, bold color"
  ...
```

`detail_help` for such a flow: ``Answer every question in one `saaio send <name> "<answer>"`; saaio
closes the dialog and the worker reads it``, in place of the `report ... working` line.

### src/cli/output/flow.rs (changed)

`sent(flow)` keeps its signature. For a flow on a dialog the first help line becomes `saaio closes
<name>'s question dialog and its worker reads this as the answer to every question in it`.

### src/cli/output/news.rs (changed)

`for_master` adds the help ``"`saaio status <name>` lists a question dialog's options"`` when a
changed flow waits on one; `says` stays the first question.

### src/server/master.md (changed)

The guided/auto lines say: a worker waiting on a question dialog gets one `saaio send` that answers
every question, by label or in words; `saaio status <name>` lists them. The current text is kept as
`master-v11.md` in `INSTRUCTIONS.earlier`.

### Docs

`docs/cli.md` (send: a dialog; status: questions and options), `docs/json.md` (`questions`),
`docs/flows.md` (answering a dialog), `ROADMAP.md` (Planned soon: send to a flow on a permission
prompt). No CHANGELOG entry before 0.1.0.

## Call chains

The worker asks:
1. Claude Code -> `commands::hook::run`: PreToolUse AskUserQuestion payload
2. hook -> `claude_code::translate(payload, workflow): Vec<EventKind>` -> `questions(input): Vec<Question>`
3. hook -> `fold::changes(id, events, kind, now): bool` -> `Store::append`
4. Error: a payload without questions gives no event; hook errors go to server.log.

The master reads it:
1. `saaio wait` wakes the master: `news::for_master` lists the flow + the options help
2. master -> `saaio status <name>` -> `fold::fold` -> `output::status::detail(flow, now, full, ..)`: questions[] and options[]

The master answers:
1. master -> `send::run(args, store, env, now)` -> `Store::append(id, Message)` -> `output::flow::sent(flow): Document` (dialog help)
2. worker's `saaio wait` -> `deliver::worker_news`: marks it delivered, exits 2; Claude Code holds it
3. `Server::serve` -> `refresh()` -> `dismiss_dialogs()`
4. `dismiss_dialogs` -> `answered_dialog(key): Option<&FlowView>` -> `Store::events(id)` -> `wake::dialog_answered(flow, events): bool`
5. `dismiss_dialogs` -> `due(typed_at, opened_at, dismissed_at, now): bool` -> `Host::write(key, ESC)`; `dismissed_at.insert`; server.log `closed <name>'s question dialog for the master's message`
6. Claude Code: tool declined, Stop hook, the held message begins a turn; the flow leaves Waiting, `questions` clear, the `dismissed_at` entry is dropped
7. The person answers in the pane first: PostToolUse -> Working, no Esc.
8. The person typed there within QUIET: wait.
9. Esc did not close it: pressed again after AGAIN.
10. `Host::write` fails: server.log; tried again after AGAIN.
11. No server running: the message waits; Esc goes in once saaio opens the pane.
12. A Codex flow never has questions: nothing pressed.

## Test seams

- `claude_code`: a two-question multiSelect fixture `pre_ask_two.json` (recorded) -> Waiting with both questions; `pre_ask.json` keeps its text.
- `event`: round trip with questions; an old line without them parses; cut_texts cuts labels.
- `fold`: questions set on a dialog, cleared by Working, Idle, Blocked, a worded Waiting; `changes` sees a new dialog.
- `wake::dialog_answered`: message after the dialog yes, before it no, no dialog no.
- `dismiss::due`: open for QUIET, QUIET since the last key, AGAIN.
- `server/tests.rs`: a raw-mode fake Claude records its bytes; a flow on a dialog plus a message gets one Esc until it closes; without a message, or on a question asked in words (all a Codex flow has), none.
- `output::status`, `output::flow`, `output::news`: exact TOON.
- `tests/cli.rs`: `saaio status` lists the options; `saaio send` prints the dialog help.

## Assumptions (approved)

1. The tables show in `saaio status <name>` too; `--full` lifts the 80-char cut.
2. The master answers in words; saaio does not check the answer against the options.
3. Esc waits for 10 s without typing in that pane (QUIET).
4. Esc again after 30 s while the dialog stays open; never more often.
5. Only a message sent after the dialog opened closes it.
6. The UI's board and detail keep showing only the first question.
7. No CHANGELOG entry before 0.1.0.

## Changes during the build

- `due` also takes `opened_at`, so no Esc goes into a harness still starting up (as `typing::ready`).
- `AGAIN` is 3 s in tests, so the server test can see that Esc is pressed only once.
- `wake::dialog_answered` (task 4) has no caller before the server task, which clippy refuses as
  dead code; tasks 4 and 7 are one commit.

## Tasks

- [x] Add `Question`, `Choice` and `Waiting.questions` in event.rs (cut_texts, serde default) and update every Waiting pattern; unit tests for the round trip, an old line and cutting
- [x] Keep every question in `claude_code::translate` via `questions(input)`, with a recorded `pre_ask_two.json` fixture and its test
- [x] Add `FlowView.questions`, set and cleared in `fold`, and in `shown`; fold tests for setting, clearing and `changes`
- [x] Add `wake::dialog_answered(flow, events)` with its unit tests
- [x] List questions and options in `output::status::detail` with the dialog help in `detail_help`; TOON tests and a `tests/cli.rs` case
- [ ] Change `output::flow::sent` and `output::news::for_master` help for a dialog; TOON tests and a `tests/cli.rs` case for send
- [x] Add `server/dismiss.rs` (`dismiss_dialogs`, `answered_dialog`, `due`, `AGAIN`, `ESC`), `Server.dismissed_at` and the call in `serve`; unit test for `due` and server tests with a raw-mode fake Claude (Esc once, none without a message, none for Codex)
- [ ] master.md: answering a dialog, with master-v11.md kept in `INSTRUCTIONS.earlier`
- [ ] Docs: docs/cli.md, docs/json.md, docs/flows.md, README if it lists send's behaviour; ROADMAP Planned soon: send to a flow on a permission prompt
- [ ] E2E: the repro in a scratch store - a guided worker asks two questions (one multiSelect), the master answers only through `saaio status` and `saaio send`, the worker gets both answers
