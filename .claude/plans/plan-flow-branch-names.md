# Branch names that follow a project's convention

Implementation page (review surface): `docs/design/branch-implementation.html`.

## Decision

- A repo commits `.flow/config.toml` with `[branch] name = "{type}/{ticket}-{slug}"` and `types = [...]`; flow
  names the branch at `flow start` (no rename later). Without the file: `flow/<name>`, as before.
- `{type}` is picked per flow: `flow start --type`, and a type row in the new-flow dialog; the first listed type
  is the default.

## Units

```rust
// src/branching.rs (new)
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Branching { pub name: String, #[serde(default)] pub types: Vec<String> }
impl Branching {
    /// <repo>/.flow/config.toml's [branch]; None without the file or the section.
    pub fn load(repo: &Path) -> Result<Option<Branching>>;
    /// `asked` when listed (error naming the list when not), else the first; None without types.
    pub fn pick_type(&self, asked: Option<&str>) -> Result<Option<String>>;
}
/// The template filled, empty placeholders' separators dropped; flow/<name> without a convention.
pub fn branch_name(branching: Option<&Branching>, kind: Option<&str>, request: &str, name: &str) -> String;

// src/launch.rs
pub fn ticket(request: &str) -> Option<&str>;   // SHOP-217 as written
pub fn slug(request: &str) -> String;           // up to 5 words after the ticket, lowercase, -joined
pub fn workspace_for(repo: &Path, name: &str, branch: String, slot: u8, agent: &str) -> Workspace;

// src/workspace.rs
pub fn branch_exists(repo: &Path, branch: &str) -> bool;

// src/main.rs
Start { .., #[arg(long = "type")] kind: Option<String> }

// src/ui
// NewFlowForm: branching: Option<Branching>, kind: usize, Field::Type between Request and Mode.
// Effect::LoadBranching(PathBuf) when the dialog opens -> ui::run -> App::on_branching(Result<Option<Branching>, String>).
// Job::Start passes --type <picked>; the preview shows branch_name(..).
```

## Call chains

1. `flow start "<request>" [--type t] -> main::start -> Branching::load(repo) -> pick_type(--type) ->
   branching::branch_name(..) -> -2, -3 while workspace::branch_exists -> launch::workspace_for(.., branch, ..) ->
   create_worktree`. A broken config, an unknown type, or `--type` without types fail before anything is created.
2. Dialog: `n` -> `Effect::LoadBranching(repo)` -> `App::on_branching` -> type row and preview -> Enter ->
   `Job::Start` -> `flow start .. --type <picked>`. A broken file shows its error under the request.

## Assumptions

1. `{ticket}` keeps the key as written; `{slug}` is up to 5 words after it, lowercased.
2. An empty placeholder drops the `-`/`_`/`.` next to it.
3. The first type is the default.
4. Only the branch follows the convention; flow names and worktree folders stay.
5. A taken branch gets `-2`, `-3` (local branches).
6. Unknown keys in `[branch]` are an error.

## Tasks

- [x] branching.rs (Branching::load, pick_type, branch_name) and launch::ticket/slug with tests
- [ ] workspace::branch_exists; workspace_for takes the branch; flow start --type with tests/cli.rs cases
- [ ] new-flow dialog: type row, LoadBranching, preview, --type in Job::Start; tests
- [ ] Docs: README, CLAUDE.md, HANDOVER
- [ ] End-to-end check: a repo with .flow/config.toml; flow start --type and the dialog create the right branch
