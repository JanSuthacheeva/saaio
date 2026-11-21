//! Tests of `saaio setup`: a flow's teardown, then its setup again, without removing it

use std::fs;

use crate::support::*;

/// Fails like composer when the marker file is missing: the cause first, the hint last.
const COMPOSER: &str = "#!/bin/sh
echo \"teardown-before-setup: $(cat \"$SAAIO_REPO/torn\" 2>/dev/null)\"
if [ ! -f \"$SAAIO_REPO/fixed\" ]; then
  echo '  - requires ext-zip' >&2
  echo '  - requires ext-bcmath' >&2
  echo 'Please run composer update'
  exit 2
fi
echo \"slot $SAAIO_SLOT\" > setup-ran
";

fn board(lab: &Lab) -> serde_json::Value {
    serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap()
}

fn started_with_a_failing_setup() -> (Lab, String) {
    let lab = Lab::new();
    lab.hook("setup", COMPOSER);
    lab.hook(
        "teardown",
        "#!/bin/sh\necho \"$SAAIO_SLOT\" > \"$SAAIO_REPO/torn\"\necho torn down\n",
    );
    lab.trust();
    let output = lab.start(&["SHOP-240 export"]);
    assert!(output.status.success(), "{}", stdout(&output));
    let log = board(&lab)["flows"][0]["setup"]["log"]
        .as_str()
        .unwrap()
        .to_string();
    (lab, log)
}

#[test]
fn a_failed_setup_keeps_its_whole_output_and_a_rerun_clears_it() {
    let (lab, log) = started_with_a_failing_setup();
    let output = fs::read_to_string(&log).unwrap();
    assert!(output.contains("requires ext-zip"), "{output}");
    assert!(output.contains("requires ext-bcmath"), "{output}");
    let status = stdout(&lab.run(&["status"]));
    assert!(
        status.contains(&format!(
            "shop-240,\"setup failed: Please run composer update (full output: {log})\""
        )),
        "{status}"
    );

    let again = lab.run(&["setup", "shop-240"]);
    assert_eq!(again.status.code(), Some(1));
    let text = stdout(&again);
    assert!(
        text.contains(&format!(
            "error: \"setup failed: Please run composer update (full output: {log})\""
        )),
        "{text}"
    );
    assert!(
        fs::read_to_string(&log)
            .unwrap()
            .contains("teardown-before-setup: 1"),
        "teardown ran first, with the flow's slot"
    );

    fs::write(lab.repo.join("fixed"), "").unwrap();
    let fixed = lab.run(&["setup", "shop-240"]);
    let text = stdout(&fixed);
    assert!(fixed.status.success(), "{text}");
    assert_eq!(
        text,
        format!(
            "flow: shop-240\nteardown: ran\nsetup: ran\nlog: {log}\nhelp[1]: Run `saaio status shop-240` to see the flow\n"
        )
    );
    let worktree = lab.repo.join(".saaio/worktrees/shop-240");
    assert_eq!(
        fs::read_to_string(worktree.join("setup-ran")).unwrap(),
        "slot 1\n"
    );
    let flow = &board(&lab)["flows"][0];
    assert_eq!(flow["state"], "idle");
    assert_eq!(flow["blocker"], serde_json::Value::Null);
    assert_eq!(flow["setup"]["failed"], serde_json::Value::Null);
    assert!(!stdout(&lab.run(&["status"])).contains("setup failed"));
}

#[test]
fn a_worker_sets_up_its_own_flow_without_naming_it() {
    let (lab, _) = started_with_a_failing_setup();
    let id = board(&lab)["flows"][0]["id"].as_str().unwrap().to_string();
    fs::write(lab.repo.join("fixed"), "").unwrap();

    let output = saaio_command(&lab.home)
        .arg("setup")
        .env("SAAIO_FLOW_ID", &id)
        .current_dir(&lab.repo)
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", stdout(&output));
    assert!(stdout(&output).starts_with("flow: shop-240\n"));
    let unnamed = lab.run(&["setup"]);
    assert_eq!(unnamed.status.code(), Some(1));
    assert!(
        stdout(&unnamed).contains("error: name the flow to set up"),
        "{}",
        stdout(&unnamed)
    );
}

#[test]
fn removing_the_flow_deletes_its_hook_logs() {
    let (lab, log) = started_with_a_failing_setup();

    let output = lab.run(&["remove", "shop-240"]);

    assert!(output.status.success(), "{}", stdout(&output));
    assert!(stdout(&output).contains("teardown: ran"));
    assert!(!std::path::Path::new(&log).parent().unwrap().exists());
}

#[test]
fn refuses_a_flow_it_cannot_set_up() {
    let lab = Lab::new();
    lab.trust();
    assert!(lab.start(&["SHOP-241 totals"]).status.success());

    let refused = |args: &[&str]| {
        let output = lab.run(args);
        assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
        stdout(&output)
    };

    assert!(
        refused(&["setup", "shop-999"]).contains("error: no flow named shop-999"),
        "unknown"
    );
    let none = refused(&["setup", "shop-241"]);
    assert!(
        none.contains(&format!(
            "error: shop-241 has no .saaio/setup in {}",
            lab.repo.display()
        )),
        "{none}"
    );
    lab.hook("setup", "#!/bin/sh\nexit 0\n");
    fs::remove_dir_all(lab.repo.join(".saaio/worktrees/shop-241")).unwrap();
    let gone = refused(&["setup", "shop-241"]);
    assert!(gone.contains("has no worktree: "), "{gone}");
}
