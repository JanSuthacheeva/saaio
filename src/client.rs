use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::proto::{self, ClientMsg, PROTOCOL_VERSION, Role, ServerMsg};
use crate::store::Store;

/// How long a new server gets to start listening.
const START_WAIT: Duration = Duration::from_secs(2);
/// How long the server gets to answer a hello.
const HELLO_WAIT: Duration = Duration::from_secs(5);

/// One connection to the session server.
pub struct Client {
    stream: UnixStream,
}

impl Client {
    /// Connects and says hello; None when no server listens; Err when it refuses (another protocol version).
    pub fn connect(socket: &Path, role: Role) -> Result<Option<Client>> {
        let mut stream = match UnixStream::connect(socket) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::NotFound | ErrorKind::ConnectionRefused
                ) =>
            {
                return Ok(None);
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not connect to {}", socket.display()));
            }
        };
        stream.set_read_timeout(Some(HELLO_WAIT))?;
        proto::write_msg(
            &mut stream,
            &ClientMsg::Hello {
                version: PROTOCOL_VERSION,
                role,
            },
        )?;
        match proto::read_msg::<_, ServerMsg>(&mut stream)
            .context("the flow server did not answer")?
        {
            Some(ServerMsg::Welcome { .. }) => {}
            Some(ServerMsg::Refused { reason }) => {
                bail!("{reason}; run `flow ui --stop` and start flow ui again")
            }
            Some(other) => bail!("the flow server answered {other:?} instead of a welcome"),
            None => bail!("the flow server closed the connection"),
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

/// <store root>/server.sock
pub fn socket_path(store: &Store) -> PathBuf {
    store.root().join("server.sock")
}

/// Starts `<exe> server` in its own session with output to server.log unless one listens;
/// waits up to 2 s for the socket.
pub fn ensure_server(socket: &Path, exe: &Path, env: &[(String, String)]) -> Result<()> {
    if UnixStream::connect(socket).is_ok() {
        return Ok(());
    }
    let log_path = socket.with_file_name("server.log");
    if let Some(dir) = log_path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("could not open {}", log_path.display()))?;
    let mut command = Command::new(exe);
    command
        .arg("server")
        .env_remove("FLOW_ID")
        .envs(env.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // SAFETY: setsid only detaches the child from this terminal's session, so closing the
    // terminal does not hang up the server; it touches no memory shared with the parent.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
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
    bail!("flow server did not start, see {}", log_path.display())
}

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;

    use tempfile::TempDir;

    use super::*;

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
                reason: "flow server runs protocol 9".into(),
            },
        );

        let error = Client::connect(&socket, Role::Control).err().unwrap();

        assert!(error.to_string().contains("protocol 9"), "{error}");
        assert!(error.to_string().contains("flow ui --stop"), "{error}");
        server.join().unwrap();
        assert!(
            Client::connect(&dir.path().join("none.sock"), Role::Control)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn leaves_a_running_server_alone_and_names_the_log_when_one_does_not_start() {
        let dir = TempDir::new().unwrap();
        let socket = dir.path().join("server.sock");
        let _listener = UnixListener::bind(&socket).unwrap();

        ensure_server(&socket, Path::new("/no/such/flow"), &[]).unwrap();

        let silent = dir.path().join("other/server.sock");
        let error = ensure_server(&silent, Path::new("/usr/bin/true"), &[]).unwrap_err();
        assert!(error.to_string().contains("server.log"), "{error}");
        assert!(dir.path().join("other/server.log").exists());
    }

    #[test]
    fn keeps_the_socket_in_the_state_directory() {
        let store = Store::at(PathBuf::from("/state/flow"));

        assert_eq!(
            socket_path(&store),
            PathBuf::from("/state/flow/server.sock")
        );
    }
}
