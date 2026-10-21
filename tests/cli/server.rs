//! Tests of `saaio server` and `saaio --stop`: stopping, its own session, the UI's prefix check

use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Instant;

use rustix::process::{Pid, Signal, getsid, kill_process};
use tempfile::TempDir;

use crate::support::*;

#[test]
fn stop_ends_a_running_server_and_says_when_none_runs() {
    let home = TempDir::new().unwrap();
    let config = cat_agents(home.path());
    let socket = home.path().join("server.sock");
    let log = home.path().join("server.log");

    let none = saaio(home.path(), &["--stop"]);
    assert!(none.status.success());
    assert_eq!(stdout(&none).trim(), "no saaio server runs");

    let server = saaio_command(home.path())
        .arg("server")
        .stdout(Stdio::null())
        .stderr(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log)
                .unwrap(),
        )
        .env("SAAIO_CONFIG", &config)
        .spawn()
        .unwrap();
    let server_pid = server.id();
    let mut server = Running(server);
    wait_for_socket(&socket);

    let stopped = saaio(home.path(), &["--stop"]);

    assert_eq!(
        stdout(&stopped).trim(),
        "stopped the saaio server and every session in it"
    );
    assert!(server.wait().unwrap().success());
    assert!(!socket.exists());
    assert!(!home.path().join("master.json").exists());

    let lines = fs::read_to_string(&log).unwrap();
    let said = |text: &str| {
        lines
            .lines()
            .find(|line| line.contains(text))
            .unwrap_or_else(|| panic!("no line with {text:?} in\n{lines}"))
    };
    let pid_of = |line: &str| line.split(['[', ']']).nth(1).unwrap().to_string();
    let server_said = format!("[{server_pid}] ");
    assert!(said("started: saaio ").contains(&server_said));
    let stopper = said(&format!("sends SIGTERM to pid {server_pid}"));
    assert!(stopper.contains("saaio --stop (saaio "), "{stopper}");
    let stopping = said("stopping: SIGTERM from pid ");
    assert!(stopping.contains(&server_said), "{stopping}");
    assert!(
        stopping.ends_with(&format!("from pid {}", pid_of(stopper))),
        "{lines}"
    );
    assert!(said("] stopped").contains(&server_said));
}

#[test]
fn a_server_stopped_while_it_starts_still_stops_cleanly() {
    let home = TempDir::new().unwrap();
    let config = cat_agents(home.path());
    let socket = home.path().join("server.sock");

    let server = saaio_command(home.path())
        .arg("server")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("SAAIO_CONFIG", &config)
        .spawn()
        .unwrap();
    let mut server = Running(server);
    // No sleep between looks: the signal has to land right after the bind.
    let deadline = Instant::now() + PATIENCE;
    while !socket.exists() {
        assert!(Instant::now() < deadline, "the server never bound");
        thread::yield_now();
    }
    let pid = Pid::from_raw(i32::try_from(server.id()).unwrap()).unwrap();
    kill_process(pid, Signal::TERM).unwrap();

    let status = server.wait().unwrap();
    assert!(status.success(), "{status:?}");
    assert!(!socket.exists());
}

#[test]
fn the_server_leads_a_session_of_its_own() {
    let home = TempDir::new().unwrap();
    let config = cat_agents(home.path());
    let socket = home.path().join("server.sock");

    let server = saaio_command(home.path())
        .arg("server")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("SAAIO_CONFIG", &config)
        .spawn()
        .unwrap();
    let mut server = Running(server);
    wait_for_socket(&socket);
    let pid = Pid::from_raw(i32::try_from(server.id()).unwrap()).unwrap();
    assert_eq!(getsid(Some(pid)).unwrap(), pid);

    kill_process(pid, Signal::TERM).unwrap();
    assert!(server.wait().unwrap().success());
}

#[test]
fn stop_refuses_inside_a_worker_on_its_own_store_and_points_to_a_scratch_home() {
    let home = TempDir::new().unwrap();
    let id = new_flow(home.path(), "shop-221");

    let refused = saaio_in(home.path(), &id, &["--stop"]);

    assert_eq!(refused.status.code(), Some(1));
    let text = stdout(&refused);
    assert!(text.contains("it would end the user's sessions"), "{text}");
    assert!(text.contains("SAAIO_HOME"), "{text}");

    let scratch = TempDir::new().unwrap();
    let elsewhere = saaio_in(scratch.path(), &id, &["--stop"]);
    assert!(elsewhere.status.success(), "{}", stdout(&elsewhere));
    assert_eq!(stdout(&elsewhere).trim(), "no saaio server runs");
}

#[test]
fn the_ui_refuses_a_prefix_the_panes_need_before_it_starts_anything() {
    let home = TempDir::new().unwrap();
    let config = home.path().join("no-config.toml");
    fs::write(&config, "[ui]\nprefix = \"C-c\"\n").unwrap();
    let opened = |said: &str| {
        let mut terminal = Terminal::open(home.path(), home.path(), &[], &[]);
        let status = terminal.child.wait().unwrap();
        wait_until(said, || terminal.shows(said));
        (status, terminal.text())
    };

    let (status, text) = opened("such as C-space or C-b");

    assert_eq!(status.exit_code(), 1);
    assert!(text.contains(&config.display().to_string()), "{text}");
    assert!(
        text.contains(
            "C-c interrupts the agent in a pane; pick another prefix, such as C-space or C-b"
        ),
        "{text}"
    );
    assert!(
        !home.path().join("server.sock").exists(),
        "no server started"
    );

    fs::write(&config, "[ui]\nprefix = \"Ctrl+b\"\n").unwrap();
    let (_, unknown) = opened("C-a to C-z");
    assert!(
        unknown.contains("unknown prefix: use C-space or C-a to C-z"),
        "{unknown}"
    );
    assert!(
        !unknown.contains("Ctrl+b"),
        "a value never comes back: {unknown}"
    );
}

#[test]
fn stop_ends_a_server_that_speaks_another_protocol() {
    let home = TempDir::new().unwrap();
    let socket = home.path().join("server.sock");
    let listen = "import socket, sys, time\n\
        server = socket.socket(socket.AF_UNIX)\n\
        server.bind(sys.argv[1])\n\
        server.listen()\n\
        time.sleep(60)\n";
    let stranger = Command::new("python3")
        .args(["-c", listen])
        .arg(&socket)
        .spawn()
        .unwrap();
    let mut stranger = Running(stranger);
    wait_for_socket(&socket);

    let stopped = saaio(home.path(), &["--stop"]);

    assert_eq!(
        stdout(&stopped).trim(),
        "stopped the saaio server and every session in it"
    );
    assert!(stranger.wait().unwrap().signal().is_some());
}
