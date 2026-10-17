use std::fs::OpenOptions;
use std::io::{self, ErrorKind};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::launch::{pane, worktree};
use crate::proto::{self, BUILD, ClientMsg, PROTOCOL_VERSION, Peer, Role, ServerMsg};
use crate::registry;
use crate::server_log;
use crate::store::Store;

/// How long a new server gets to start listening.
const START_WAIT: Duration = Duration::from_secs(2);
/// How long the server gets to answer a hello.
const HELLO_WAIT: Duration = Duration::from_secs(5);
/// How long a stopped server gets to close its panes (3.5 s) and exit before SIGKILL.
const STOP_WAIT: Duration = Duration::from_secs(5);
/// How long a killed server gets to stop listening.
const KILL_WAIT: Duration = Duration::from_secs(1);

/// What became of a flow's pane when `saaio remove` or `saaio clear` asked the server to close it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closing {
    Closed,
    NoServer,
    NoPane,
    TimedOut,
}

/// One connection to the session server.
#[derive(Debug)]
pub struct Client {
    stream: UnixStream,
}

impl Client {
    /// Connects and says hello; None when no server listens; Err when it refuses (another protocol version).
    pub fn connect(socket: &Path, role: Role) -> Result<Option<Client>> {
        let Some(mut stream) = dial(socket)? else {
            return Ok(None);
        };
        stream.set_read_timeout(Some(HELLO_WAIT))?;
        proto::write_msg(
            &mut stream,
            &ClientMsg::Hello {
                version: PROTOCOL_VERSION,
                role,
                peer: this_process(),
            },
        )?;
        match proto::read_msg::<_, ServerMsg>(&mut stream)
            .context("the saaio server did not answer")?
        {
            Some(ServerMsg::Welcome { .. }) => {}
            Some(ServerMsg::Refused { reason }) => {
                bail!("{reason}; run `saaio ui --stop` and start saaio ui again")
            }
            Some(other) => bail!("the saaio server answered {other:?} instead of a welcome"),
            None => bail!("the saaio server closed the connection"),
        }
        // macOS refuses socket options once the server hung up (EINVAL); the next recv says so.
        let _ = stream.set_read_timeout(None);
        Ok(Some(Client { stream }))
    }

    pub fn send(&mut self, message: &ClientMsg) -> Result<()> {
        proto::write_msg(&mut self.stream, message)
    }

    /// The next server message; None when the server closed the connection.
    pub fn recv(&mut self) -> Result<Option<ServerMsg>> {
        proto::read_msg(&mut self.stream)
    }

    /// A second handle on the same socket, so one thread reads while another writes.
    pub fn try_clone(&self) -> Result<Client> {
        Ok(Client {
            stream: self.stream.try_clone()?,
        })
    }
}

/// This process as a Peer: its pid, current folder (empty when it cannot be read) and BUILD.
fn this_process() -> Peer {
    Peer {
        pid: std::process::id(),
        cwd: std::env::current_dir().unwrap_or_default(),
        build: BUILD.to_string(),
    }
}

/// None when no server listens on `socket`.
fn dial(socket: &Path) -> Result<Option<UnixStream>> {
    match UnixStream::connect(socket) {
        Ok(stream) => Ok(Some(stream)),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::NotFound | ErrorKind::ConnectionRefused
            ) =>
        {
            Ok(None)
        }
        Err(error) => {
            Err(error).with_context(|| format!("could not connect to {}", socket.display()))
        }
    }
}

/// Stops the server on `socket` with SIGTERM, SIGKILL when it still listens after STOP_WAIT;
/// false when none runs. A signal, not a message, so it stops a server of any protocol version.
/// Each step goes into server.log, as the server cannot log a SIGKILL or who sent it.
pub fn stop_server(socket: &Path, clock: fn() -> u64) -> Result<bool> {
    let Some(stream) = dial(socket)? else {
        return Ok(false);
    };
    let pid = peer_pid(&stream).with_context(|| {
        format!(
            "could not tell which process listens on {}",
            socket.display()
        )
    })?;
    drop(stream);
    let log = server_log::path(socket);
    // A log that cannot be written never keeps the server from stopping.
    let note = |text: String| {
        let _ = server_log::append(&log, clock(), &text);
    };
    note(format!(
        "saaio ui --stop (saaio {BUILD}) sends SIGTERM to pid {pid}"
    ));
    for (signal, wait) in [(libc::SIGTERM, STOP_WAIT), (libc::SIGKILL, KILL_WAIT)] {
        if signal == libc::SIGKILL {
            note(format!(
                "pid {pid} still listens after {} s; sends SIGKILL",
                STOP_WAIT.as_secs()
            ));
        }
        // SAFETY: kill only sends a signal; a pid that already exited fails with ESRCH.
        unsafe {
            libc::kill(pid, signal);
        }
        if stops_listening(socket, wait) {
            return Ok(true);
        }
    }
    note(format!("pid {pid} did not stop"));
    bail!("the saaio server (pid {pid}) did not stop")
}

fn stops_listening(socket: &Path, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    while UnixStream::connect(socket).is_ok() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    true
}

/// The pid of the process at the other end of a Unix socket.
#[cfg(target_os = "macos")]
fn peer_pid(stream: &UnixStream) -> io::Result<libc::pid_t> {
    let mut pid: libc::pid_t = 0;
    let mut len = libc::socklen_t::try_from(size_of::<libc::pid_t>()).expect("a pid fits");
    // SAFETY: getsockopt writes at most len bytes into pid, which is that big.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&raw mut pid).cast(),
            &raw mut len,
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(pid)
}

/// The pid of the process at the other end of a Unix socket.
#[cfg(target_os = "linux")]
fn peer_pid(stream: &UnixStream) -> io::Result<libc::pid_t> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = libc::socklen_t::try_from(size_of::<libc::ucred>()).expect("a ucred fits");
    // SAFETY: getsockopt writes at most len bytes into cred, which is that big.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut cred).cast(),
            &raw mut len,
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(cred.pid)
}

/// Asks a running server to end a flow's pane and waits for it; the outcome when there is no server or no pane.
pub fn close_flow(socket: &Path, id: &str, timeout: Duration) -> Result<Closing> {
    let Some(mut client) = Client::connect(socket, Role::Control)? else {
        return Ok(Closing::NoServer);
    };
    client.send(&ClientMsg::CloseFlow(id.to_string()))?;
    let deadline = Instant::now() + timeout;
    loop {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(Closing::TimedOut);
        };
        client.stream.set_read_timeout(Some(left))?;
        match client.recv() {
            Ok(Some(ServerMsg::Closed(closed))) if closed == id => return Ok(Closing::Closed),
            Ok(Some(ServerMsg::NotOpen(open))) if open == id => return Ok(Closing::NoPane),
            Ok(Some(_)) => {}
            Ok(None) => return Ok(Closing::Closed),
            Err(error) if timed_out(&error) => return Ok(Closing::TimedOut),
            Err(error) => return Err(error),
        }
    }
}

fn timed_out(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| matches!(io.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
    })
}

/// Unix sockets paths must be shorter than this (104 bytes on macOS, 108 on Linux).
const SOCKET_PATH_LIMIT: usize = 100;

/// <store root>/server.sock; when that is too long for a socket, /tmp/saaio-<uid>/<hash of the root>.sock.
pub fn socket_path(store: &Store) -> PathBuf {
    socket_path_for(store.root(), Path::new("/tmp"))
}

fn socket_path_for(root: &Path, tmp: &Path) -> PathBuf {
    let near = root.join("server.sock");
    if near.as_os_str().len() < SOCKET_PATH_LIMIT {
        return near;
    }
    // SAFETY: getuid cannot fail and touches no memory.
    let uid = unsafe { libc::getuid() };
    tmp.join(format!("saaio-{uid}")).join(format!(
        "{:016x}.sock",
        registry::fnv1a(root.as_os_str().as_encoded_bytes())
    ))
}

/// `<exe> server` with `env` and without the hook variables or SAAIO_MASTER of the session it starts
/// from, so a saaio ui opened inside a worker or the master gives its panes none of that identity.
fn server_command(exe: &Path, env: &[(String, String)]) -> Command {
    let mut command = Command::new(exe);
    command.arg("server");
    for var in worktree::HOOK_VARS.iter().chain(&[pane::MASTER_ENV]) {
        command.env_remove(var);
    }
    command.envs(env.iter().map(|(key, value)| (key, value)));
    command
}

/// Starts `<exe> server` in its own session with output to server.log unless one listens, moving a
/// log past server_log::CAP to server.log.1 first; waits up to 2 s for the socket.
pub fn ensure_server(socket: &Path, exe: &Path, env: &[(String, String)]) -> Result<()> {
    if UnixStream::connect(socket).is_ok() {
        return Ok(());
    }
    let log_path = server_log::path(socket);
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("could not restrict {}", dir.display()))?;
    }
    // A log that cannot be moved aside is only longer; it never keeps the server from starting.
    let _ = server_log::rotate(&log_path, server_log::CAP);
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("could not open {}", log_path.display()))?;
    let mut command = server_command(exe, env);
    command
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // A session of its own, so closing this terminal does not hang up the server.
    // SAFETY: pre_exec runs between fork and exec, where only async-signal-safe calls are
    // allowed; setsid is one, and the closure allocates nothing.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
        .spawn()
        .with_context(|| format!("could not start {} server", exe.display()))?;
    let deadline = Instant::now() + START_WAIT;
    while Instant::now() < deadline {
        if UnixStream::connect(socket).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
    bail!("saaio server did not start, see {}", log_path.display())
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn starts_the_server_without_the_identity_of_the_session_it_starts_from() {
        let command = server_command(
            Path::new("/bin/saaio"),
            &[("SAAIO_HOME".to_string(), "/state".to_string())],
        );

        let envs: Vec<(String, Option<String>)> = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect();
        for var in [
            "SAAIO_FLOW_ID",
            "SAAIO_FLOW_NAME",
            "SAAIO_SLOT",
            "SAAIO_WORKTREE",
            "SAAIO_REPO",
        ] {
            assert!(envs.contains(&(var.to_string(), None)), "{var}: {envs:?}");
        }
        assert!(envs.contains(&("SAAIO_HOME".to_string(), Some("/state".to_string()))));
    }

    #[test]
    fn knows_the_process_at_the_other_end_of_a_socket() {
        let dir = TempDir::new().unwrap();
        let socket = dir.path().join("server.sock");
        let _listener = UnixListener::bind(&socket).unwrap();

        let stream = UnixStream::connect(&socket).unwrap();

        let pid = peer_pid(&stream).unwrap();
        assert_eq!(u32::try_from(pid).unwrap(), std::process::id());
    }

    /// Answers one hello with `reply`, then keeps the connection open until dropped.
    fn fake_server(socket: &Path, reply: ServerMsg) -> thread::JoinHandle<Option<ClientMsg>> {
        let listener = UnixListener::bind(socket).unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let hello = proto::read_msg::<_, ClientMsg>(&mut stream).unwrap();
            proto::write_msg(&mut stream, &reply).unwrap();
            hello
        })
    }

    #[test]
    fn says_hello_and_is_welcomed() {
        let dir = TempDir::new().unwrap();
        let socket = dir.path().join("server.sock");
        let server = fake_server(
            &socket,
            ServerMsg::Welcome {
                version: PROTOCOL_VERSION,
            },
        );

        let client = Client::connect(&socket, Role::Control).unwrap();

        assert!(client.is_some());
        assert_eq!(
            server.join().unwrap(),
            Some(ClientMsg::Hello {
                version: PROTOCOL_VERSION,
                role: Role::Control,
                peer: this_process(),
            })
        );
    }

    #[test]
    fn passes_on_a_refusal_and_finds_no_server_where_none_listens() {
        let dir = TempDir::new().unwrap();
        let socket = dir.path().join("server.sock");
        let server = fake_server(
            &socket,
            ServerMsg::Refused {
                reason: "saaio server runs protocol 9".into(),
            },
        );

        let error = Client::connect(&socket, Role::Control).err().unwrap();

        assert!(error.to_string().contains("protocol 9"), "{error}");
        assert!(error.to_string().contains("saaio ui --stop"), "{error}");
        server.join().unwrap();
        assert!(
            Client::connect(&dir.path().join("none.sock"), Role::Control)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn moves_a_full_log_aside_before_it_starts_a_server() {
        let dir = TempDir::new().unwrap();
        let socket = dir.path().join("server.sock");
        let log = dir.path().join("server.log");
        let full = vec![b'x'; usize::try_from(server_log::CAP).unwrap() + 1];
        std::fs::write(&log, &full).unwrap();

        let _ = ensure_server(&socket, Path::new("/usr/bin/true"), &[]);

        assert_eq!(
            std::fs::read(dir.path().join("server.log.1")).unwrap(),
            full
        );
        assert_eq!(std::fs::metadata(&log).unwrap().len(), 0);
    }

    #[test]
    fn leaves_a_running_server_alone_and_names_the_log_when_one_does_not_start() {
        let dir = TempDir::new().unwrap();
        let socket = dir.path().join("server.sock");
        let _listener = UnixListener::bind(&socket).unwrap();

        ensure_server(&socket, Path::new("/no/such/saaio"), &[]).unwrap();

        let silent = dir.path().join("other/server.sock");
        let error = ensure_server(&silent, Path::new("/usr/bin/true"), &[]).unwrap_err();
        assert!(error.to_string().contains("server.log"), "{error}");
        assert!(dir.path().join("other/server.log").exists());
        let mode = std::fs::metadata(dir.path().join("other"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn moves_a_socket_whose_path_is_too_long_into_a_private_tmp_directory() {
        let long = PathBuf::from(format!("/state/{}", "deep/".repeat(30)));

        let socket = socket_path_for(&long, Path::new("/tmp"));

        assert!(socket.as_os_str().len() < SOCKET_PATH_LIMIT);
        assert!(socket.starts_with("/tmp"), "{}", socket.display());
        assert!(
            socket
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("saaio-")
        );
        assert_eq!(socket, socket_path_for(&long, Path::new("/tmp")));
        assert_ne!(
            socket,
            socket_path_for(&long.join("other"), Path::new("/tmp"))
        );
    }

    #[test]
    fn keeps_the_socket_in_the_state_directory() {
        let store = Store::at(PathBuf::from("/state/saaio"));

        assert_eq!(
            socket_path(&store),
            PathBuf::from("/state/saaio/server.sock")
        );
    }

    #[test]
    fn starts_the_server_without_the_identity_of_the_session_it_comes_from() {
        let command = server_command(
            Path::new("/bin/saaio"),
            &[("SAAIO_HOME".into(), "/state".into())],
        );
        let envs: Vec<(String, Option<String>)> = command
            .get_envs()
            .map(|(key, value)| {
                let value = value.map(|value| value.to_string_lossy().into_owned());
                (key.to_string_lossy().into_owned(), value)
            })
            .collect();

        for var in worktree::HOOK_VARS.iter().chain(&[pane::MASTER_ENV]) {
            assert!(
                envs.contains(&(var.to_string(), None)),
                "{var} removed: {envs:?}"
            );
        }
        assert!(envs.contains(&("SAAIO_HOME".into(), Some("/state".into()))));
    }
}
