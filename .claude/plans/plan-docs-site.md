# Docs site at saaio.dev

Review pages: approaches https://claude.ai/artifact/V2sCYqmLwBcqhuH1AWm3vd (`docs/design/docs-site-approaches.html`),
implementation https://claude.ai/artifact/9vkwBewuEb9YBnF36Ycem3 (`docs/design/docs-site-implementation.html`).

## Decision

- Our own generator: a binary `saaio-site` in `src/bin/site/`, built only with `--features site`
  (`required-features`), so `cargo install` never builds it. A second Cargo package is not an
  option: `cargo install --git` searches the whole repo and refuses one with two packages that
  have binaries (tried in a scratch repo).
- It renders `README.md` (home), `docs/README.md` and the docs pages, `CHANGELOG.md` and
  `ROADMAP.md` with pulldown-cmark (already a dependency, `html` feature on; no new crate),
  gives headings GitHub's slugs, rewrites and checks every link, and wraps each page in a
  hand-written jasu.dev frame. Pagefind (pinned binary, SHA-256 checked) indexes the output.
- `.github/workflows/docs.yml` builds on every PR (check `site`) and on main also deploys with
  `actions/upload-pages-artifact` + `actions/deploy-pages` to GitHub Pages; custom domain
  saaio.dev, DNS at Cloudflare, DNS only.
- Theme follows the device (`prefers-color-scheme`), with a toggle remembered in localStorage.
- URLs: `/` (README), `/docs/` (docs/README.md), `/docs/<page>/`, `/changelog/`, `/roadmap/`.
- Every image a page shows (the 1.9 MB GIF too) is copied to its repo path on the site.
- The Laravel-style rewrite of the docs is a separate flow after this one; ROADMAP gets its row.
- Alternative noted, not built: Cloudflare Pages deployed from Actions (`wrangler pages deploy`),
  which would add PR previews at the cost of a Cloudflare token in the repo secrets.

## Units

`Cargo.toml` (modified):

```toml
[features]
# The docs site generator, built in CI and for a local preview, never by cargo install.
site = []

[[bin]]
name = "saaio"
path = "src/main.rs"

[[bin]]
name = "saaio-site"
path = "src/bin/site/main.rs"
required-features = ["site"]
```

`src/bin/site/main.rs` - the process boundary: parse, build, print, exit code.

```rust
#[derive(clap::Parser)]
struct Args {
    /// The checkout to read.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// The folder to write the site to; empty, missing, or an earlier build.
    out: PathBuf,
}

fn main() -> ExitCode; // 0 built, 1 broken links or an I/O error, 2 usage
```

Success prints `built 15 pages and 2 assets into _site` to stdout; failure prints each `Broken`
or the error to stderr.

`src/bin/site/site.rs` - one whole build; everything renders before anything is written.

```rust
pub struct Built { pub pages: usize, pub assets: usize }

pub enum Failure {
    Broken(Vec<Broken>),
    Io(anyhow::Error),
}

pub fn build(root: &Path, out: &Path) -> Result<Built, Failure>;

/// The anchors a link names that its page does not have.
fn missing_anchors(rendered: &[(&Page, Rendered)]) -> Vec<Broken>;
```

`src/bin/site/pages.rs` - the pages the site publishes, in nav order.

```rust
pub struct Page {
    pub source: PathBuf, // repo-relative: docs/ui.md
    pub url: String,     // /docs/ui/
    pub title: String,   // the page's first # heading
}

pub struct Pages { list: Vec<Page> }

impl Pages {
    /// README.md, docs/README.md, the docs pages in the order docs/README.md's table links
    /// them, then CHANGELOG.md and ROADMAP.md. Errors when a docs/*.md other than
    /// UNPUBLISHED is missing from that table.
    pub fn read(root: &Path) -> anyhow::Result<Pages>;
    pub fn by_source(&self, source: &Path) -> Option<&Page>;
    pub fn iter(&self) -> impl Iterator<Item = &Page>;
}

/// README.md -> "/", docs/README.md -> "/docs/", docs/ui.md -> "/docs/ui/",
/// CHANGELOG.md -> "/changelog/", ROADMAP.md -> "/roadmap/".
fn url_of(source: &Path) -> String;

const UNPUBLISHED: &[&str] = &["docs/HANDOVER.md"];
```

`src/bin/site/render.rs` - one page's Markdown to its HTML body, through pulldown-cmark events.

```rust
pub struct Rendered {
    pub body: String,
    pub outline: Vec<Heading>,    // its ## headings, for "On this page"
    pub anchors: HashSet<String>, // every heading id
    pub links: Vec<Link>,         // links into pages, anchors checked later
    pub assets: Vec<PathBuf>,     // repo files copied to the site
}

pub struct Heading { pub level: u8, pub text: String, pub id: String }

/// Options: tables, strikethrough, task lists, GitHub alerts (ENABLE_GFM). Headings get
/// Slugs ids, links and images go through links::resolve, tables sit in a scroll box, the
/// leading # heading stays the page's h1. Errors list every link whose file is missing.
pub fn render(page: &Page, markdown: &str, pages: &Pages, root: &Path)
    -> Result<Rendered, Vec<Broken>>;

/// The text of the first # heading.
pub fn title(markdown: &str) -> Option<String>;
```

`src/bin/site/links.rs` - where one Markdown link points on the site.

```rust
pub const REPO: &str = "https://github.com/JanSuthacheeva/saaio";

pub enum Target {
    /// A published page: its url plus "#anchor".
    Page { source: PathBuf, url: String, anchor: Option<String> },
    /// An image file (png, gif, jpg, jpeg, svg, webp) in the repo, linked or shown,
    /// copied to the same path on the site.
    Asset { source: PathBuf },
    /// Any other repo file or folder: {REPO}/blob/main/<path> or {REPO}/tree/main/<path>.
    Repo { url: String },
    /// http(s), mailto: and the like, as written.
    External(String),
}

impl Target {
    pub fn href(&self) -> Cow<'_, str>;
}

/// Resolves href against the folder of `from`. "#x" is the page itself; "/x" starts at the
/// checkout, as on GitHub.
pub fn resolve(from: &Path, href: &str, pages: &Pages, root: &Path) -> Result<Target, Reason>;

pub struct Link { pub line: usize, pub href: String, pub target: Target }

pub struct Broken { pub source: PathBuf, pub line: usize, pub href: String, pub reason: Reason }

pub enum Reason {
    NoFile,
    /// The page it names has no heading with that id.
    NoAnchor(PathBuf),
    OutsideRepo,
}

impl fmt::Display for Broken; // docs/ui.md:12: configuration.md#nope: no heading #nope in docs/configuration.md
```

`src/bin/site/slug.rs` - GitHub's heading ids.

```rust
pub struct Slugs { seen: HashMap<String, usize> }

impl Slugs {
    pub fn new() -> Slugs;
    /// Lowercased; characters other than letters, digits, "-", "_" and spaces dropped;
    /// spaces to "-"; a repeat gets "-1", "-2".
    pub fn next(&mut self, text: &str) -> String;
}
```

`src/bin/site/layout.rs` with `page.html`, `site.css`, `site.js` beside it - the page frame.

```rust
const TEMPLATE: &str = include_str!("page.html");
pub const CSS: &str = include_str!("site.css");
pub const JS: &str = include_str!("site.js");

/// page.html with {{title}}, {{nav}} (this page marked current), {{body}}, {{outline}} and
/// {{edit}} ({REPO}/edit/main/{source}) filled in one pass.
pub fn page(page: &Page, pages: &Pages, rendered: &Rendered) -> String;

/// 404.html: the frame with "Page not found" and the nav.
pub fn not_found(pages: &Pages) -> String;

fn fill(template: &str, values: &[(&str, &str)]) -> String;
```

Frame: header (wordmark `saaio`, search box, theme toggle, GitHub link), left nav, content
(`data-pagefind-body`), "On this page" outline, footer (Edit this page, licence). Under 900px the
nav folds behind a menu button; the outline hides under 1200px. Head: inline script reading
`localStorage.theme` (try/catch) into `data-theme` before paint; `/pagefind/pagefind-ui.css`,
`/pagefind/pagefind-ui.js`. Tokens from `~/projects/dotfiles-v2/agents/design-system/tokens.json`:
light on `:root`, dark under `@media (prefers-color-scheme: dark) :root:not([data-theme="light"])`
and `:root[data-theme="dark"]`. Pagefind UI themed through its CSS variables. GitHub alerts
(`blockquote.markdown-alert-*`) styled as callouts with their label.

`src/bin/site/output.rs` - the binary's one file that creates files, behind
`#[allow(clippy::disallowed_methods, reason = "the site is public output, not the user's private files")]`.

```rust
pub struct Output { dir: PathBuf }

impl Output {
    /// Empties dir, or creates it. Refuses a non-empty folder without the .saaio-site marker.
    pub fn create(dir: &Path) -> anyhow::Result<Output>;
    /// "/docs/ui/" -> dir/docs/ui/index.html
    pub fn page(&self, url: &str, html: &str) -> anyhow::Result<()>;
    pub fn file(&self, path: &str, text: &str) -> anyhow::Result<()>;
    pub fn copy(&self, root: &Path, source: &Path) -> anyhow::Result<()>;
}
```

`.github/workflows/docs.yml` (new):

```yaml
on: { pull_request, push: { branches: [main] }, workflow_dispatch }
permissions: { contents: read }
concurrency: { group: pages-${{ github.ref }}, cancel-in-progress: false }
jobs:
  site:     # every PR and main: build, link check, Pagefind; on main also configure-pages + upload-pages-artifact (path: _site)
  deploy:   # main only, needs: site; permissions pages: write, id-token: write; environment github-pages; actions/deploy-pages
```

Actions pinned to commits with a `# vX` comment, `timeout-minutes` on each job, as in ci.yml.

Modified: `ci.yml` (clippy `--all-targets --all-features`, test `--all-features`), `.gitignore`
(`/_site`), `CLAUDE.md` (build commands, the binary, the cargo install constraint),
`CONTRIBUTING.md` (Docs site section: local preview, deploy, the docs/README.md row, link check,
the manual setup; CI table gains `site`; `--all-features` commands), `README.md` (Documentation
opens with "Read online at saaio.dev"), `ROADMAP.md` (docs site row out; "Docs in the Laravel
style" row in), `CHANGELOG.md` (Unreleased, Added).

## Data

Site on disk: `/index.html`, `/docs/index.html`, `/docs/<page>/index.html`, `/changelog/`,
`/roadmap/`, `/docs/media/saaio-ui.png`, `/docs/media/saaio-ui.gif`, `/site.css`, `/site.js`,
`/404.html`, `/pagefind/` (from Pagefind), `/.saaio-site`.

## Call chains

`saaio-site _site`:

1. `main -> Args::parse(): Args` (usage error exits 2)
2. `main -> site::build(&root, &out): Result<Built, Failure>`
3. `site::build -> Pages::read(root): anyhow::Result<Pages>` (calls `render::title` per file)
4. per page: `site::build -> render::render(page, markdown, &pages, root): Result<Rendered, Vec<Broken>>`
5. per heading `Slugs::next(text): String`; per link or image `links::resolve(from, href, &pages, root): Result<Target, Reason>`
6. `site::build -> missing_anchors(&rendered): Vec<Broken>`
7. `site::build -> Output::create(out)`, per page `Output::page(&url, &layout::page(..))`, per asset `Output::copy`, `Output::file` for site.css, site.js, 404.html, .saaio-site
8. `main` prints the summary, exit 0

Errors: a docs page missing from the docs/README.md table -> `Pages::read` error, exit 1. Missing
file or a path leaving the checkout -> `Broken { NoFile | OutsideRepo }` collected across all
pages, exit 1, nothing written. Missing heading -> `Broken { NoAnchor }`, exit 1. Non-empty
unmarked out folder -> `Output::create` refuses, exit 1, nothing removed.

CI: PR and main -> `site` job: `cargo run --locked --features site --bin saaio-site -- _site`
(exit 1 fails the check) -> download Pagefind, `sha256sum -c`, `pagefind --site _site` -> on main
`upload-pages-artifact` -> `deploy` job `deploy-pages`. A failed deploy leaves the previous build
served. `ci.yml` `cargo test --all-features` runs `the_repository_builds`.

Reader: head script applies a stored theme; the toggle flips `data-theme` against the shown theme
and stores it; `new PagefindUI({ element: "#search", showSubResults: true })`.

## By hand (Jan)

Cloudflare DNS for saaio.dev, every record DNS only (grey cloud): A `@` 185.199.108.153,
185.199.109.153, 185.199.110.153, 185.199.111.153; AAAA `@` 2606:50c0:8000::153,
2606:50c0:8001::153, 2606:50c0:8002::153, 2606:50c0:8003::153; CNAME `www`
jansuthacheeva.github.io; TXT `_github-pages-challenge-JanSuthacheeva` with GitHub's code. SSL/TLS
mode Full (strict) as a guard. Remove parking records; a CAA record must allow letsencrypt.org.

GitHub: verify saaio.dev (account Settings -> Pages -> Add a domain); repo Settings -> Pages ->
Source: GitHub Actions; after the first deploy, Custom domain saaio.dev, then Enforce HTTPS;
environment `github-pages` deploys from main only. No CNAME file in the build.

## Test seams

- `slug`: GitHub's slugs of real headings (`` `[ui]` `` -> `ui`, `saaio --stop` -> `saaio---stop`, repeats).
- `links`: tempfile checkout: page, anchor-only, `../examples`, folder, image, external, missing, outside.
- `render`: heading ids, alert markup, table wrapper, rewritten hrefs, Broken line numbers.
- `pages`: nav order from a scratch docs/README.md, unlisted page errors, `url_of`.
- `output`: refuses a non-empty unmarked folder, replaces a marked one.
- `layout`: placeholders filled, current page marked, outline present.
- `site::tests::the_repository_builds`: the real checkout (`env!("CARGO_MANIFEST_DIR")`) builds with no Broken.

## Assumptions (approved)

1. CHANGELOG.md and ROADMAP.md are pages.
2. README.md is the home page as written; version pins stay in the Markdown.
3. Nav order and membership from the docs/README.md table; a missing page fails the build.
4. No syntax highlighting.
5. The site builds on every PR (`site` check, and `cargo test --all-features`).
6. No web fonts: the design system's system stacks.
7. "Edit this page" per page.
8. Pagefind pinned to the latest 1.x at least two weeks old, SHA-256 checked.

## Tasks

### Generator

- [x] Cargo.toml `site` feature and `[[bin]]` entries; `src/bin/site/main.rs` with `Args` and a stub build; `.gitignore` `/_site`; ci.yml `--all-features` for clippy and test
- [x] `slug.rs`: `Slugs::new`, `Slugs::next` with unit tests on real headings and repeats
- [x] `pages.rs`: `Page`, `Pages::read`, `by_source`, `iter`, `url_of`, `render::title` with tests for nav order, unlisted page, urls
- [x] `links.rs`: `Target`, `Target::href`, `resolve`, `Link`, `Broken`, `Reason`, Display, with tempfile tests for every target kind and failure
- [ ] `render.rs`: `render`, `Rendered`, `Heading` with tests for ids, alerts, tables, hrefs, Broken lines
- [ ] `output.rs`: `Output::create`, `page`, `file`, `copy` with marker tests
- [ ] `layout.rs` with `page.html`, `site.css`, `site.js` in the jasu.dev look: `page`, `not_found`, `fill`, tests for placeholders and the current page
- [ ] `site.rs`: `build`, `missing_anchors`, `Built`, `Failure`; main prints and maps exit codes; `the_repository_builds` test; fix any broken link it finds in the docs

### Deploy and docs

- [ ] `.github/workflows/docs.yml` with pinned actions and Pagefind checksum
- [ ] CLAUDE.md, CONTRIBUTING.md (Docs site section, CI table, manual setup), README Documentation, ROADMAP rows, CHANGELOG line
- [ ] End to end: build, `pagefind --site _site --serve`, check in a browser at desktop and phone width, light and dark, toggle remembered, search, links across pages and anchors, the GIF, 404; `cargo install --path .` still installs only `saaio`
