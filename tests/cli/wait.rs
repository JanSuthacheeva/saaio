use std::fs;
use std::time::Duration;

use tempfile::TempDir;

use crate::support::*;

#[test]
fn a_workers_wait_ends_with_the_masters_message_once() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c0000-0000-4000-8000-000000000000",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"waiting","question":"Which delimiter?"}"#,
    );
    let short = Duration::from_millis(1200);
    let mut first = waiter(
        home.path(),
        Some("7f3c0000-0000-4000-8000-000000000000"),
        &[],
    );
    assert!(ended(&mut first, short).is_none(), "nothing to deliver yet");

    saaio(home.path(), &["send", "shop-217", "Semicolons."]);

    let (code, out, err) = ended(&mut first, PATIENCE).expect("woken");
    assert_eq!(code, 2);
    assert_eq!(out, "");
    assert_eq!(
        err.trim_end(),
        "message from the saaio master:\n  Semicolons."
    );
    assert_eq!(
        fs::read_to_string(
            home.path()
                .join("waits/7f3c0000-0000-4000-8000-000000000000.busy")
        )
        .unwrap(),
        "turn",
        "the turn it wakes runs no prompt hook"
    );

    let mut again = waiter(
        home.path(),
        Some("7f3c0000-0000-4000-8000-000000000000"),
        &[],
    );
    assert!(ended(&mut again, short).is_none(), "delivered once only");
    let replacing = waiter(
        home.path(),
        Some("7f3c0000-0000-4000-8000-000000000000"),
        &[],
    );
    let (code, _, err) = ended(&mut again, PATIENCE).expect("replaced");
    assert_eq!((code, err.as_str()), (0, ""));
    drop(replacing);
}

#[test]
fn the_masters_wait_ends_with_the_flows_that_stopped() {
    let home = TempDir::new().unwrap();
    seed_worker(
        home.path(),
        "7f3c0000-0000-4000-8000-000000000000",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"waiting","question":"Which delimiter?"}"#,
    );
    seed_worker(
        home.path(),
        "8a1b",
        "shop-218",
        r#"{"v":1,"at":2000,"kind":"working"}"#,
    );

    let mut master = waiter(home.path(), None, &["--master"]);
    let (code, _, err) = ended(&mut master, PATIENCE).expect("woken");

    assert_eq!(code, 2);
    assert!(
        err.starts_with(
            "changed[1]{name,state,mode,says}:\n  shop-217,waiting,auto,Which delimiter?\n"
        ),
        "{err}"
    );

    let mut again = waiter(home.path(), None, &["--master"]);
    assert!(
        ended(&mut again, Duration::from_secs(3)).is_none(),
        "it saw shop-217 already"
    );
    fs::write(
        home.path().join("flows/8a1b.jsonl"),
        fs::read_to_string(home.path().join("flows/8a1b.jsonl")).unwrap()
            + r#"{"v":1,"at":3000,"kind":"idle","note":"Plan is ready."}"#
            + "\n",
    )
    .unwrap();
    let (code, _, err) = ended(&mut again, PATIENCE).expect("woken again");
    assert_eq!(code, 2);
    assert!(err.contains("shop-218,idle,auto,Plan is ready."), "{err}");
    assert!(!err.contains("shop-217"), "{err}");
}

#[test]
fn the_masters_wait_brings_no_news_while_it_does_not_wake() {
    let home = TempDir::new().unwrap();
    let config = home.path().join("no-config.toml");
    fs::write(&config, "[master]\nwake = false\n").unwrap();
    seed_worker(
        home.path(),
        "7f3c0000-0000-4000-8000-000000000000",
        "shop-217",
        r#"{"v":1,"at":2000,"kind":"waiting","question":"Which delimiter?"}"#,
    );

    let mut master = waiter(home.path(), None, &["--master"]);
    assert!(
        ended(&mut master, Duration::from_secs(4)).is_none(),
        "no news while wake is false"
    );
    fs::write(&config, "[master]\nwake = true\n").unwrap();
    let (code, _, err) = ended(&mut master, PATIENCE).expect("woken once it wakes");

    assert_eq!(code, 2);
    assert!(err.contains("shop-217,waiting"), "{err}");
}

#[test]
fn a_wait_with_nobody_to_wait_for_ends_quietly() {
    let home = TempDir::new().unwrap();
    let mut nobody = waiter(home.path(), None, &[]);
    assert_eq!(
        ended(&mut nobody, PATIENCE),
        Some((0, String::new(), String::new()))
    );
    let mut gone = waiter(home.path(), Some("nope"), &[]);
    assert_eq!(
        ended(&mut gone, PATIENCE),
        Some((0, String::new(), String::new()))
    );
}
