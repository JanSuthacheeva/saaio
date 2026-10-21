# Update notice and `saaio update`

Review pages: approaches https://claude.ai/artifact/HqFBxqM2Y1Qq4s9r32dGpc (`docs/design/update-approaches.html`),
implementation https://claude.ai/artifact/XijZVPGzA6kp2faaJt3CLT (`docs/design/update-implementation.html`).

## Decision

- The UI checks the latest release itself, on a thread, through `curl`, reading a new release asset
  `VERSION` (the tag) at `<releases>/latest/download/VERSION`. GitHub's "latest" skips pre-releases
  and drafts. At most once a day across every UI on the machine: `update.json` in the shared folder
  records the last look and the latest tag found. Turned off with `[ui] update_check = false`.
- `release.yml` builds with `SAAIO_RELEASE=<tag>`; `build.rs` passes it on as a rustc env. A binary
  without it is a source build: `saaio update` names the `cargo install` line instead of replacing it.
- `saaio update [--version TAG]` resolves the tag, checks the binary's folder is writable, fetches
  that release's `install.sh` and runs it (`sh -s`, script on stdin) with `SAAIO_VERSION` and
  `SAAIO_INSTALL_DIR`, so the checksum check and the rename stay in install.sh.
- `SAAIO_RELEASES_URL` (public, documented as a mirror of the releases page) replaces
  `https://github.com/JanSuthacheeva/saaio/releases`; tests point it at a `file://` folder.
- New dependency: `semver`. rustix gains the `fs` feature (`access`).

## Units

### src/release.rs (new)

```rust
pub mod source;

/// A release's tag, v0.1.1 or v0.1.2-rc.1.
pub struct Tag { version: semver::Version }
impl Tag {
    pub fn parse(text: &str) -> Result<Tag>;   // "v0.1.1" or "0.1.1"; else an error naming it
    pub fn version(&self) -> &semver::Version;
}
impl Display for Tag;                          // v0.1.1
impl Ord for Tag;                              // semver: 0.1.1-rc.1 < 0.1.1 < 0.1.2
impl Serialize, Deserialize for Tag;           // as its Display string

pub enum Build { Release(Tag), Source(semver::Version) }
impl Build {
    pub fn running() -> Build;                 // env!("SAAIO_RELEASE"), else CARGO_PKG_VERSION
    fn of(stamp: &str, package: &str) -> Build;
    pub fn version(&self) -> &semver::Version;
}
```

### src/release/source.rs (new)

```rust
pub const GITHUB: &str = "https://github.com/JanSuthacheeva/saaio/releases";

pub struct Releases { root: String }
impl Releases {
    pub fn at(root: Option<String>) -> Releases;            // GITHUB unless set and non-empty
    pub fn url(&self, tag: Option<&Tag>, asset: &str) -> String;
        // <root>/latest/download/<asset> or <root>/download/<tag>/<asset>
    pub fn latest(&self, limit: Duration) -> Result<Tag>;   // VERSION of the latest release
    pub fn fetch(&self, tag: &Tag, asset: &str, limit: Duration) -> Result<String>;
    pub fn install_env(&self, tag: &Tag) -> Vec<(String, String)>;
        // SAAIO_VERSION always; SAAIO_DOWNLOAD_URL=<root>/download/<tag> when root is not GITHUB
    fn is_github(&self) -> bool;
    fn curl(&self, url: &str, limit: Duration) -> Result<String>;
        // curl -fsSL --connect-timeout 3 --max-time <limit>, plus --proto =https --tlsv1.2 for GITHUB,
        // through workspace::process::output
}
```

### src/update.rs (new)

```rust
pub const EVERY_MS: u64 = 24 * 60 * 60 * 1000;
pub const FILE: &str = "update.json";

#[derive(Serialize, Deserialize)]
pub struct Checked { pub at: u64, pub latest: Option<Tag> }

pub struct UpdateCheck { file: PathBuf }
impl UpdateCheck {
    pub fn in_folder(registry: &Path) -> UpdateCheck;
    pub fn last(&self) -> Option<Checked>;                     // None when missing or unreadable
    pub fn due(&self, now: u64) -> bool;                       // no look within EVERY_MS
    pub fn record(&self, now: u64, latest: Option<Tag>) -> io::Result<()>;
        // None keeps the latest an earlier look found; private::write_atomic
}

pub fn notice(running: &Build, latest: &Tag) -> Option<String>; // "0.1.1 is out: saaio update"

pub enum Step { Latest, Source, Unwritable, Install }
pub fn step(running: &Build, target: &Tag, asked: bool, writable: bool) -> Step;
    // Source first; then Latest when !asked and target <= running; then Unwritable; else Install
```

### src/ui/update_watch.rs (new)

```rust
pub struct UpdateWatch { check: Option<UpdateCheck>, releases: Releases, running: Build, enabled: bool, asking: bool }
impl UpdateWatch {
    pub fn new(check: Option<UpdateCheck>, releases: Releases, running: Build) -> UpdateWatch;
    pub fn set_enabled(&mut self, enabled: bool);
    pub fn ask_if_due(&mut self, now: u64, events: &Sender<Event>); // records, then a thread
    pub fn answered(&mut self, now: u64, latest: Option<Tag>);
    pub fn notice(&self) -> Option<String>;                         // None when off
}
const LIMIT: Duration = Duration::from_secs(5);
```

### Modified

```rust
// src/ui.rs
enum Event { ..., Latest(Option<Tag>) }
pub struct UiEnv { ..., update: Option<UpdateCheck>, releases: Releases }
const TICKS_PER_UPDATE: u32 = 36_000; // an hour
// reload_config also sets updates.set_enabled(config.ui.update_check)

// src/ui/app.rs
update: Option<String>,
pub fn set_update(&mut self, line: Option<String>) -> bool;

// src/ui/app/chrome.rs: right side = notice, else update line + rail hint;
// update line: "0.1.1 is out: " faint, "saaio update" key style; dropped when the left would keep
// fewer than 20 columns.
fn update_line(&self, theme: &Theme) -> Vec<Span<'static>>;

// src/config.rs
pub struct UiConfig { pub prefix: Prefix, pub update_check: bool } // true unless set
// examples/config.toml: #update_check = true, explained

// src/cli.rs: Command::Update(update::Args), dispatched before the store
// src/cli/env.rs: pub saaio_releases_url: Option<String>; pub(crate) fn releases(&self) -> Releases
// src/main.rs: SAAIO_RELEASES_URL
// src/cli/commands/ui.rs: UiEnv { update: env.registry().map(|r| UpdateCheck::in_folder(r.root())), releases: env.releases() }
// src/workspace/process.rs: pub fn output(argv: &[String], limit: Duration) -> Result<String>

// src/cli/commands/update.rs
pub struct Args { #[arg(long, value_name = "TAG")] version: Option<String> }
pub fn run(args: &Args, env: &Env) -> Result<Document, Failure>;
fn install(releases: &Releases, tag: &Tag, dir: &Path) -> anyhow::Result<()>; // tests run the installed binary

// Build: Env::releases and SAAIO_RELEASES_URL came with the UI task, which needed them first;
// UpdateWatch is a private module of ui, as it takes ui's private Event.

// src/cli/output/update.rs
pub fn updated(from: &Build, to: &Tag, binary: &str) -> Document;
pub fn latest(running: &Build, latest: &Tag) -> Document;
pub fn source(to: &Tag) -> Failure;
pub fn unwritable(dir: &str, releases: &Releases, to: &Tag) -> Failure;

// build.rs: cargo:rustc-env=SAAIO_RELEASE=<$SAAIO_RELEASE or "">, rerun-if-env-changed=SAAIO_RELEASE
// release.yml: Build step env SAAIO_RELEASE=$GITHUB_REF_NAME (Test step without it);
//   package writes release/VERSION; install-check reads it back
```

## Data

- `<shared folder>/update.json`, 0600: `{"at":1792569600000,"latest":"v0.1.1"}`.
- Release asset `VERSION`: the tag and a newline.
- `saaio update` TOON:
  ```
  updated: saaio 0.1.0 -> 0.1.1
  binary: ~/.local/bin/saaio
  help[2]: A running saaio server keeps 0.1.0 until `saaio --stop`; unfinished flows resume after,Changes: https://github.com/JanSuthacheeva/saaio/releases/tag/v0.1.1
  ```

## Call chains

UI start and hourly:

1. `ui::run -> UpdateWatch::new(env.update, env.releases, Build::running())`
2. `reload_config -> UpdateWatch::set_enabled(config.ui.update_check)`; off -> `App::set_update(None)`
3. `ui::run -> App::set_update(watch.notice())` from update.json, no network
4. `ui::run -> UpdateWatch::ask_if_due(now, &events)` at start and each TICKS_PER_UPDATE
5. `UpdateWatch -> UpdateCheck::due(now)`: false -> done
6. `UpdateWatch -> UpdateCheck::record(now, None)` before asking
7. thread: `Releases::latest(LIMIT)` -> `Event::Latest(Some(tag))`, any error -> `Event::Latest(None)`
8. `ui::run -> UpdateWatch::answered(now, latest) -> UpdateCheck::record`; `App::set_update(watch.notice())`

Errors: offline, timeout, 404, no curl, file errors -> nothing shows, nothing logged.

`saaio update [--version TAG]`:

1. `cli::run -> commands::update::run(args, env)`, without a store
2. `Tag::parse(version)` (error: exit 2, names the value) or `env.releases().latest(15 s)` (error:
   exit 1 "could not read the latest release: <why>", help: check the connection or pass --version)
3. `current_exe()?.canonicalize()`, its folder; `rustix::fs::access(dir, Access::WRITE_OK)`
4. `update::step(&Build::running(), &tag, asked, writable)`
5. Latest -> `output::update::latest`, exit 0
6. Source -> `output::update::source`: `cargo install --locked --git https://github.com/JanSuthacheeva/saaio --tag <tag> --force`, exit 1
7. Unwritable -> `output::update::unwritable`: `curl -fsSL <install.sh url> | sudo env SAAIO_VERSION=<tag> SAAIO_INSTALL_DIR=<dir> sh`, exit 1
8. Install -> `install`: `Releases::fetch(tag, "install.sh")` -> `sh -s` with `install_env` and
   `SAAIO_INSTALL_DIR`; a failure -> exit 1 with install.sh's last line
9. `output::update::updated(from, to, binary)`

## Test seams

- release.rs: Tag parse and ordering, Build::of.
- release/source.rs: url; latest and fetch against a `file://` folder; a missing folder errs.
- update.rs: due, record keeping an earlier tag, notice, step matrix.
- update_watch.rs: off shows nothing; answered sets the notice; ask_if_due sends Latest from a fake folder.
- chrome: golden files for the update line with the sidebar, with the rail, dropped when narrow.
- commands/update.rs `install`: real install.sh against a fake release folder replacing a binary in a temp folder; a bad checksum fails.
- tests/cli/update.rs: source build names the cargo line; latest unreachable fails with help; a bad --version fails.

## Assumptions

1. A failed look counts as the day's look.
2. No key dismisses the notice.
3. No newer release: nothing happens; `--version` installs that tag even if older or the same.
4. A source build is never replaced by a release binary, even with `--version`.
5. The notice shows for source builds too.
6. `SAAIO_RELEASES_URL` is public, documented as a mirror of the releases page.
7. The update line drops first when the title row is short.
8. The ROADMAP row is removed, as it ships here.

## Tasks

- [x] Add `release::Tag` and `release::Build` with `semver`, `build.rs` passing `SAAIO_RELEASE`; unit tests for parse, ordering and `Build::of`
- [x] Add `workspace::process::output` and `release::source::Releases` (url, latest, fetch, install_env, curl) with tests against a `file://` folder
- [x] Add `update::UpdateCheck`, `Checked`, `notice` and `step` with unit tests (due, record keeping an earlier tag, notice text, step matrix)
- [x] Add `[ui] update_check` to `UiConfig` and `#update_check = true` to `examples/config.toml`; config tests pass
- [x] Add `App::set_update` and the update line in `chrome.rs`; golden files with the sidebar, the rail and a narrow row
- [x] Add `ui::update_watch::UpdateWatch`, `Event::Latest`, `UiEnv` fields and the hourly look in `ui.rs`; unit tests for off, answered and ask_if_due
- [x] Add `Env::saaio_releases_url`, `Env::releases`, `Command::Update`, `commands::update::{run, install}` and `output::update`; tests for install with the real install.sh, and `tests/cli/update.rs`
- [x] `release.yml`: `SAAIO_RELEASE` on the Build step, `VERSION` asset, install-check reads it
- [ ] Docs: `docs/cli.md` (saaio update, SAAIO_RELEASES_URL), `docs/install.md` Updating, `docs/configuration.md` [ui] and the one network call, `docs/files.md` update.json, ROADMAP row removed, CHANGELOG Added, CLAUDE.md module lines; design pages into `docs/design/`
- [ ] End to end: the UI in a PTY with a scratch store and a fake releases folder shows `9.9.9 is out: saaio update`, and `update_check = false` hides it; `saaio update` from a source build names the cargo line
