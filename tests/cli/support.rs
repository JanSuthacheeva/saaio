//! What several CLI subjects share: saaio with a test's own state, repos, hooks, waiters

use std::fs;
use std::io::{Read, Write};
use std::ops::{Deref, DerefMut};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, MasterPty, PtySize, SlavePty, native_pty_system};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

/// The saaio binary with a test's own state: `home` as SAAIO_HOME, the shared folder inside it, a
/// config file that does not exist (so the built-in agents), no Claude Code home of the user's
/// and no SAAIO_* variable of the shell it runs in.
pub fn saaio_command(home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_saaio"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("SAAIO_") {
            command.env_remove(key);
        }
    }
    command
        .env("SAAIO_HOME", home)
        .env("XDG_STATE_HOME", shared(home))
        .env("SAAIO_CONFIG", home.join("no-config.toml"))
        .env("XDG_CONFIG_HOME", home.join("no-config-home"))
        .env("CLAUDE_CONFIG_DIR", home.join("no-claude-home"))
        .env(
            "PATH",
            path_first(Path::new(env!("CARGO_BIN_EXE_saaio")).parent().unwrap()),
        );
    command
}

/// PATH with `dir` first, as an install puts saaio's folder: a saaio of another build earlier on
/// PATH makes the UI and `saaio update` warn.
pub fn path_first(dir: &Path) -> std::ffi::OsString {
    let rest = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&rest)))
        .unwrap()
}

pub fn saaio(home: &Path, args: &[&str]) -> Output {
    saaio_command(home)
        .args(args)
        .output()
        .expect("the saaio binary runs")
}

pub fn saaio_in(home: &Path, flow_id: &str, args: &[&str]) -> Output {
    saaio_command(home)
        .args(args)
        .env("SAAIO_FLOW_ID", flow_id)
        .env("SAAIO_PROOF", key(home, flow_id))
        .output()
        .expect("the saaio binary runs")
}

/// `saaio <args>` in the master's session: with the key its pane was handed.
pub fn as_master(home: &Path, args: &[&str]) -> Output {
    saaio_command(home)
        .args(args)
        .env("SAAIO_PROOF", key(home, "master"))
        .output()
        .expect("the saaio binary runs")
}

/// The key the pane of `name` (a flow id, or master) proves its session with, its digest kept in
/// the store as saaio server keeps it.
pub fn key(home: &Path, name: &str) -> String {
    let key = format!("{name}-key");
    let waits = home.join("waits");
    fs::create_dir_all(&waits).unwrap();
    let digest = Sha256::digest(key.as_bytes());
    fs::write(waits.join(format!("{name}.key")), format!("{digest:x}")).unwrap();
    key
}

/// git in `dir`, which must succeed.
pub fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// How long a test waits for what should happen soon; only a failing test waits this long.
pub const PATIENCE: Duration = Duration::from_secs(30);

/// Polls `done` until it holds; fails the test with `what` after PATIENCE.
pub fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        thread::sleep(Duration::from_millis(20));
    }
}

/// A config file in `home` that runs cat for the worker and the master; its path.
pub fn cat_agents(home: &Path) -> PathBuf {
    let config = home.join("config.toml");
    fs::write(
        &config,
        "[agents.claude]\ncommand = [\"cat\"]\nprompt = \"\"\nresume = [\"cat\"]\n\n[agents.master]\ncommand = [\"cat\"]\nprompt = \"\"\nresume = [\"cat\"]\n",
    )
    .unwrap();
    config
}

/// Waits until something accepts connections on `socket`; a socket file can exist before that.
pub fn wait_for_socket(socket: &Path) {
    wait_until("the server never listened", || {
        UnixStream::connect(socket).is_ok()
    });
}

/// A saaio process the test started, killed when the test ends, also when it fails.
pub struct Running(pub Child);

impl Deref for Running {
    type Target = Child;

    fn deref(&self) -> &Child {
        &self.0
    }
}

impl DerefMut for Running {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn new_flow(home: &Path, name: &str) -> String {
    let output = saaio(
        home,
        &[
            "new",
            "--name",
            name,
            "--request",
            "SHOP-221 add CSV export",
            "--mode",
            "auto",
        ],
    );
    assert!(output.status.success(), "{}", stdout(&output));
    id_in(&stdout(&output))
}

/// The id a document prints; TOON quotes one that starts like a number (02e1...).
pub fn id_in(text: &str) -> String {
    text.lines()
        .find_map(|line| line.strip_prefix("id: "))
        .expect("the document prints an id")
        .trim_matches('"')
        .to_string()
}

/// Stops the server of a test's state when the test ends, also one that failed.
pub struct StopsServer<'a>(pub &'a Path);

impl Drop for StopsServer<'_> {
    fn drop(&mut self) {
        let _ = saaio(self.0, &["--stop"]);
    }
}

/// The shared folder every project registers in: inside the test's home, never the real one.
pub fn shared(home: &Path) -> PathBuf {
    home.join("shared")
}

pub fn stdout(output: &Output) -> String {
    std::str::from_utf8(&output.stdout).unwrap().to_string()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

pub fn seed(home: &Path, id: &str, lines: &[&str]) {
    let flows = home.join("flows");
    fs::create_dir_all(&flows).unwrap();
    fs::write(flows.join(format!("{id}.jsonl")), lines.join("\n") + "\n").unwrap();
}

/// Bare `saaio` in a real terminal, as a person runs it: the test's own state, in `dir`, with
/// no agent's shell around it and `env` added.
pub struct Terminal {
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    screen: Arc<Mutex<Vec<u8>>>,
    _master: Box<dyn MasterPty + Send>,
    /// Held open: macOS drops what the child wrote but nobody read yet once the last slave closes.
    _slave: Box<dyn SlavePty + Send>,
}

impl Terminal {
    pub fn open(home: &Path, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Terminal {
        let template = saaio_command(home);
        let mut command = CommandBuilder::new(template.get_program());
        command.args(args);
        for (key, value) in template.get_envs() {
            match value {
                Some(value) => command.env(key, value),
                None => command.env_remove(key),
            }
        }
        command.env_remove("CLAUDECODE");
        command.env_remove("CODEX_THREAD_ID");
        command.env("TERM", "xterm-256color");
        for (key, value) in env {
            command.env(key, value);
        }
        command.cwd(dir);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let child = pair.slave.spawn_command(command).unwrap();
        let screen = Arc::new(Mutex::new(Vec::new()));
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();
        let shown = Arc::clone(&screen);
        thread::spawn(move || {
            let mut buf = [0; 4096];
            while let Ok(n @ 1..) = reader.read(&mut buf) {
                let mut screen = shown.lock().unwrap();
                let asked = |screen: &[u8]| screen.windows(4).filter(|w| w == b"\x1b[6n").count();
                let before = asked(&screen);
                screen.extend_from_slice(&buf[..n]);
                // Answers where the cursor is, as a terminal does, so the UI starts.
                for _ in before..asked(&screen) {
                    let _ = writer.write_all(b"\x1b[1;1R");
                }
            }
        });
        Terminal {
            child,
            screen,
            _master: pair.master,
            _slave: pair.slave,
        }
    }

    pub fn shows(&self, text: &str) -> bool {
        String::from_utf8_lossy(&self.screen.lock().unwrap()).contains(text)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.screen.lock().unwrap()).to_string()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `saaio server` with cat for the worker and the master and `gh` from `script` first on PATH;
/// its stderr in home/server.log.
pub fn server_with_gh(home: &Path, script: &str) -> Running {
    let config = cat_agents(home);
    let bin = home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    executable(&bin.join("gh"), script);
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let log = fs::File::create(home.join("server.log")).unwrap();
    let server = saaio_command(home)
        .arg("server")
        .stdout(Stdio::null())
        .stderr(log)
        .env("SAAIO_CONFIG", &config)
        .env("PATH", path)
        .spawn()
        .unwrap();
    let server = Running(server);
    wait_for_socket(&home.join("server.sock"));
    server
}

/// A temp git repo with one commit and a state folder.
pub struct Lab {
    pub dir: TempDir,
    pub repo: PathBuf,
    pub home: PathBuf,
}

impl Lab {
    pub fn new() -> Lab {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let repo = root.join("repo");
        let home = root.join("state");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        Lab { dir, repo, home }
    }

    pub fn start(&self, args: &[&str]) -> Output {
        let mut all = vec!["start"];
        all.extend_from_slice(args);
        self.run(&all)
    }

    /// Runs saaio in the repo with the lab's state.
    pub fn run(&self, args: &[&str]) -> Output {
        saaio_command(&self.home)
            .args(args)
            .current_dir(&self.repo)
            .output()
            .unwrap()
    }

    /// Runs saaio in the repo with the lab's state, as the session of flow `flow_id`.
    pub fn run_as(&self, flow_id: &str, args: &[&str]) -> Output {
        saaio_command(&self.home)
            .args(args)
            .env("SAAIO_FLOW_ID", flow_id)
            .env("SAAIO_PROOF", key(&self.home, flow_id))
            .current_dir(&self.repo)
            .output()
            .unwrap()
    }

    /// The claude agent saaio runs, as config.toml lines.
    pub fn agent(&self, lines: &str) {
        fs::create_dir_all(&self.home).unwrap();
        fs::write(
            self.home.join("no-config.toml"),
            format!("[agents.claude]\n{lines}\n[agents.master]\n{lines}\n"),
        )
        .unwrap();
    }

    /// `saaio server` in the repo, once it listens.
    pub fn server(&self) -> Running {
        let server = saaio_command(&self.home)
            .arg("server")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .current_dir(&self.repo)
            .spawn()
            .unwrap();
        let server = Running(server);
        wait_for_socket(&self.home.join("server.sock"));
        server
    }

    /// The flow branches in the repo, sorted.
    pub fn branches(&self) -> Vec<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(["branch", "--list", "flow/*", "--format=%(refname:short)"])
            .output()
            .unwrap();
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(String::from)
            .collect()
    }

    /// A commit in the flow's worktree, on its branch only.
    pub fn commit_in(&self, name: &str) {
        let worktree = self.repo.join(".saaio/worktrees").join(name);
        fs::write(worktree.join("work.txt"), "done").unwrap();
        git(&worktree, &["add", "work.txt"]);
        git(
            &worktree,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "-m",
                "work",
            ],
        );
    }

    /// `saaio trust` in the repo, run in a terminal as the person runs it.
    pub fn trust(&self) {
        let mut terminal = Terminal::open(&self.home, &self.repo, &["trust"], &[]);
        wait_until("saaio trust did not end", || {
            terminal.child.try_wait().unwrap().is_some()
        });
        assert!(terminal.shows("trusted: "), "{}", terminal.text());
    }

    pub fn hook(&self, name: &str, script: &str) {
        let dir = self.repo.join(".saaio");
        fs::create_dir_all(&dir).unwrap();
        executable(&dir.join(name), script);
    }
}

/// Writes an executable script through a child process, so this process never holds it open for
/// writing while another test thread forks: exec of such a file fails with ETXTBSY on Linux.
pub fn executable(path: &Path, body: &str) {
    let mut writer = Command::new("sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    writer
        .stdin
        .take()
        .unwrap()
        .write_all(body.as_bytes())
        .unwrap();
    let status = writer.wait().unwrap();
    assert!(status.success(), "could not write {}", path.display());
}

/// Runs `saaio hook claude-code` with a hook payload on stdin, as Claude Code does.
pub fn hook(home: &Path, flow_id: Option<&str>, payload: &str) -> Output {
    hook_with(home, &["claude-code"], flow_id, payload)
}

/// Runs `saaio hook <args>` with a hook payload on stdin.
pub fn hook_with(home: &Path, args: &[&str], flow_id: Option<&str>, payload: &str) -> Output {
    feed(hook_command(home, args, flow_id), payload)
}

/// `saaio hook <args>` for the flow `flow_id`, not yet run.
pub fn hook_command(home: &Path, args: &[&str], flow_id: Option<&str>) -> Command {
    let mut command = saaio_command(home);
    command.arg("hook").args(args);
    if let Some(id) = flow_id {
        command
            .env("SAAIO_FLOW_ID", id)
            .env("SAAIO_PROOF", key(home, id));
    }
    command
}

/// Runs `command` with `payload` on stdin.
pub fn feed(mut command: Command, payload: impl AsRef<[u8]>) -> Output {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let written = child.stdin.take().unwrap().write_all(payload.as_ref());
    // A hook that refuses before it reads may exit before the payload is written.
    if let Err(error) = written {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    child.wait_with_output().unwrap()
}

pub fn seed_worker(home: &Path, id: &str, name: &str, last: &str) {
    let started = format!(
        r#"{{"v":1,"at":1000,"kind":"started","name":"{name}","request":"x","mode":"auto"}}"#
    );
    let workspace = format!(
        r#"{{"v":1,"at":1500,"kind":"workspace","path":"/repo/.saaio/worktrees/{name}","branch":"flow/{name}","slot":1,"window":"saaio-ui","agent":"claude"}}"#
    );
    seed(home, id, &[&started, &workspace, last]);
}

pub fn waiter(home: &Path, flow_id: Option<&str>, args: &[&str]) -> Running {
    let mut command = saaio_command(home);
    command
        .arg("wait")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(id) = flow_id {
        command
            .env("SAAIO_FLOW_ID", id)
            .env("SAAIO_PROOF", key(home, id));
    }
    if args.contains(&"--master") {
        command.env("SAAIO_PROOF", key(home, "master"));
    }
    Running(command.spawn().expect("the saaio binary runs"))
}

/// The waiter's exit code, stdout and stderr once it ends; None while it still runs after `limit`.
pub fn ended(child: &mut Child, limit: Duration) -> Option<(i32, String, String)> {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            let mut out = String::new();
            let mut err = String::new();
            child
                .stdout
                .take()
                .unwrap()
                .read_to_string(&mut out)
                .unwrap();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut err)
                .unwrap();
            return Some((status.code().unwrap_or(-1), out, err));
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}
