//! Tests of `saaio trust`: granted only in a person's terminal, revoked and listed anywhere

use crate::support::*;

#[test]
fn only_a_person_at_a_terminal_trusts_a_repository() {
    let lab = Lab::new();
    lab.hook("setup", "#!/bin/sh\nexit 0\n");

    let refused = lab.run(&["trust"]);

    let text = stdout(&refused);
    assert_eq!(refused.status.code(), Some(1), "{text}");
    assert!(text.contains("only a person trusts a repository"), "{text}");
    let mut agent = Terminal::open(&lab.home, &lab.repo, &["trust"], &[("CLAUDECODE", "1")]);
    wait_until("saaio trust did not end", || {
        agent.child.try_wait().unwrap().is_some()
    });
    assert!(agent.shows("only a person"), "{}", agent.text());
    assert!(stdout(&lab.run(&["trust", "--list"])).starts_with("trusted: []\n"));

    lab.trust();

    let listed = stdout(&lab.run(&["trust", "--list"]));
    assert!(
        listed.starts_with(&format!("trusted[1]: {}\n", lab.repo.display())),
        "{listed}"
    );
}

#[test]
fn trusts_the_repository_a_subfolder_is_in_and_names_its_scripts() {
    let lab = Lab::new();
    lab.hook("setup", "#!/bin/sh\nexit 0\n");
    lab.hook("teardown", "#!/bin/sh\nexit 0\n");
    std::fs::create_dir(lab.repo.join("src")).unwrap();
    let mut terminal = Terminal::open(&lab.home, &lab.repo.join("src"), &["trust"], &[]);
    wait_until("saaio trust did not end", || {
        terminal.child.try_wait().unwrap().is_some()
    });

    assert!(
        terminal.shows(&format!("trusted: {}", lab.repo.display())),
        "{}",
        terminal.text()
    );
    assert!(
        terminal.shows("scripts[2]: .saaio/setup,.saaio/teardown"),
        "{}",
        terminal.text()
    );
}

#[test]
fn revokes_the_trust_so_start_asks_again() {
    let lab = Lab::new();
    lab.hook("setup", "#!/bin/sh\nexit 0\n");
    lab.trust();

    let revoked = stdout(&lab.run(&["trust", "--revoke"]));
    let again = stdout(&lab.run(&["trust", "--revoke", "."]));

    assert!(
        revoked.starts_with(&format!("revoked: {}\n", lab.repo.display())),
        "{revoked}"
    );
    assert!(again.starts_with("revoked: none\n"), "{again}");
    let start = lab.start(&["SHOP-217 add CSV export"]);
    assert_eq!(start.status.code(), Some(1));
    assert!(stdout(&start).contains("is not trusted"));
}

#[test]
fn revokes_the_trust_of_a_folder_that_is_gone() {
    let lab = Lab::new();
    let other = lab.home.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let other = other.canonicalize().unwrap();
    let mut terminal = Terminal::open(&lab.home, &other, &["trust"], &[]);
    wait_until("saaio trust did not end", || {
        terminal.child.try_wait().unwrap().is_some()
    });
    assert!(terminal.shows("trusted: "), "{}", terminal.text());
    std::fs::remove_dir(&other).unwrap();

    let revoked = lab.run(&["trust", "--revoke", other.to_str().unwrap()]);

    let text = stdout(&revoked);
    assert_eq!(revoked.status.code(), Some(0), "{text}");
    assert!(
        text.starts_with(&format!("revoked: {}\n", other.display())),
        "{text}"
    );
    assert!(stdout(&lab.run(&["trust", "--list"])).starts_with("trusted: []\n"));
}
