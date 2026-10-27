//! Tests of `saaio start`: worktrees, setup, slots, `--plain`, branch names and the server

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Output;

use tempfile::TempDir;

use crate::support::*;

#[test]
fn start_makes_a_worktree_runs_setup_and_leaves_the_worker_to_saaio_ui() {
    let lab = Lab::new();
    lab.hook(
        "setup",
        "#!/bin/sh\necho \"slot $SAAIO_SLOT\" > setup-ran\n",
    );
    lab.trust();

    let output = lab.start(&["SHOP-217 add CSV export"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    let worktree = lab.repo.join(".saaio/worktrees/shop-217");
    assert!(text.starts_with("flow: shop-217\n"), "{text}");
    assert!(
        text.contains(&format!("worktree: {}", worktree.display())),
        "{text}"
    );
    assert!(
        text.contains(
            "branch: flow/shop-217\nslot: 1\nsetup: ran\nagent: claude\npane: when saaio opens"
        ),
        "{text}"
    );
    assert!(
        text.contains("Run `saaio` in a terminal to open the worker"),
        "{text}"
    );
    assert_eq!(
        fs::read_to_string(worktree.join("setup-ran"))
            .unwrap()
            .trim(),
        "slot 1"
    );

    let board: serde_json::Value =
        serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap();
    assert_eq!(
        board["flows"][0]["workspace"]["window"],
        serde_json::Value::Null
    );
    assert_eq!(board["flows"][0]["workspace"]["slot"], 1);
    let log = fs::read_dir(lab.home.join("flows"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .unwrap();
    assert!(
        fs::read_to_string(log)
            .unwrap()
            .contains(r#""phases":["plan","implement","review","merge-request"]"#),
        "start records the workflow"
    );
}

#[test]
fn start_hands_the_worker_to_a_running_saaio_server() {
    let lab = Lab::new();
    lab.agent(
        r#"command = ["cat"]
prompt = ""
resume = ["cat"]"#,
    );
    let mut server = lab.server();

    let output = lab.start(&["SHOP-217 add CSV export"]);

    let text = stdout(&output);
    assert!(text.contains("pane: opening"), "{text}");
    assert!(
        text.contains("saaio opens shop-217 in its sidebar"),
        "{text}"
    );
    let hosted = || {
        let board: serde_json::Value =
            serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap();
        board["flows"][0]["workspace"]["window"] == "saaio-ui"
    };
    wait_until("the server never opened the worker", hosted);
    assert!(lab.run(&["--stop"]).status.success());
    assert!(server.wait().unwrap().success());
}

#[test]
fn a_configured_agent_that_exits_at_launch_blocks_status_and_json() {
    let lab = Lab::new();
    fs::create_dir_all(&lab.home).unwrap();
    fs::write(
        lab.home.join("no-config.toml"),
        r#"[worker]
agent = "dead"
[master]
agent = "master"
[agents.dead]
command = ["sh", "-c", "printf 'first\\nlast\\n'; exit 23"]
prompt = ""
[agents.master]
command = ["cat"]
prompt = ""
"#,
    )
    .unwrap();
    let mut server = lab.server();
    assert!(
        lab.start(&["--name", "dead", "repro worker exit"])
            .status
            .success()
    );

    wait_until("exited worker stayed working", || {
        let output = lab.run(&["status", "dead", "--json"]);
        let board: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        board["flows"][0]["state"] == "blocked"
    });
    let output = lab.run(&["status", "dead"]);
    let shown = stdout(&output);
    assert!(shown.contains("state: blocked"), "{shown}");
    assert!(shown.contains("code 23"), "{shown}");
    assert!(shown.contains("first"), "{shown}");
    assert!(shown.contains("last"), "{shown}");
    let board: serde_json::Value =
        serde_json::from_slice(&lab.run(&["status", "dead", "--json"]).stdout).unwrap();
    assert_eq!(board["flows"][0]["state"], "blocked");
    assert_eq!(
        board["flows"][0]["blocker"],
        "agent exited (code 23)\nfirst\nlast"
    );

    assert!(lab.run(&["--stop"]).status.success());
    assert!(server.wait().unwrap().success());
}

#[test]
fn status_says_where_a_flows_code_is_and_whether_its_worktree_is_left() {
    let lab = Lab::new();
    assert!(lab.start(&["SHOP-217 add CSV export"]).status.success());
    let worktree = lab.repo.join(".saaio/worktrees/shop-217");
    assert!(
        lab.run(&["report", "--flow", "shop-217", "ended"])
            .status
            .success()
    );

    let ended = stdout(&lab.run(&["status", "shop-217"]));

    assert!(
        ended.contains(&format!(
            "branch: flow/shop-217\nworktree: {}\nworktree_exists: true\n",
            worktree.display()
        )),
        "{ended}"
    );
    assert!(ended.contains("C-space r in saaio resumes it"), "{ended}");
    let config = lab.home.join("no-config.toml");
    fs::write(&config, "[ui]\nprefix = \"C-b\"\n").unwrap();
    let ended = stdout(&lab.run(&["status", "shop-217"]));
    assert!(ended.contains("C-b r in saaio resumes it"), "{ended}");
    fs::write(&config, "[ui]\nprefix = \"C-c\"\n").unwrap();
    let broken = lab.run(&["status", "shop-217"]);
    assert!(
        broken.status.success(),
        "a config the UI refuses leaves status working"
    );
    assert!(stdout(&broken).contains("C-space r in saaio resumes it"));
    fs::remove_file(&config).unwrap();
    git(
        &lab.repo,
        &["worktree", "remove", worktree.to_str().unwrap()],
    );
    let gone = stdout(&lab.run(&["status", "shop-217"]));
    assert!(gone.contains("worktree_exists: false"), "{gone}");
    assert!(gone.contains("its commits stay on flow/shop-217"), "{gone}");
}

#[test]
fn after_a_crash_the_server_resumes_the_master_and_every_worker() {
    let lab = Lab::new();
    let log = lab.home.join("agents.log");
    lab.agent(&format!(
        r#"command = ["sh", "-c", "echo fresh {{name}} >> {log}; exec cat"]
prompt = ""
resume = ["sh", "-c", "echo resumed {{name}} >> {log}; exec cat"]"#,
        log = log.display()
    ));
    let lines = || {
        let mut lines: Vec<String> = fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect();
        lines.sort();
        lines
    };
    let wait_for = |count: usize| {
        wait_until("the agents never all ran", || lines().len() >= count);
    };
    let mut server = lab.server();
    assert!(lab.start(&["SHOP-1 first"]).status.success());
    assert!(lab.start(&["SHOP-2 second"]).status.success());
    wait_for(3);
    assert_eq!(lines(), ["fresh master", "fresh shop-1", "fresh shop-2"]);

    server.kill().unwrap();
    server.wait().unwrap();
    let mut again = lab.server();
    wait_for(6);

    assert_eq!(
        lines()[3..],
        ["resumed master", "resumed shop-1", "resumed shop-2"]
    );
    assert!(lab.run(&["--stop"]).status.success());
    assert!(again.wait().unwrap().success());
}

#[test]
fn the_worker_gets_the_whole_request_however_long() {
    let lab = Lab::new();
    let prompts = lab.home.join("prompts");
    fs::create_dir_all(&prompts).unwrap();
    lab.agent(&format!(
        r#"command = ["sh", "-c", "printf %s \"$1\" > {dir}/{{name}}; exec cat", "sh", "{{prompt}}"]
prompt = "{{request}}"
resume = ["cat"]"#,
        dir = prompts.display()
    ));
    let request = format!("SHOP-300 {} the tail", "word ".repeat(120));
    let mut server = lab.server();

    assert!(lab.start(&[&request]).status.success());

    let prompt = || fs::read_to_string(prompts.join("shop-300")).unwrap_or_default();
    wait_until("the worker never ran", || !prompt().is_empty());
    assert_eq!(prompt(), request);
    let board: serde_json::Value =
        serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap();
    assert_eq!(board["flows"][0]["request"], request.as_str());
    assert!(lab.run(&["--stop"]).status.success());
    assert!(server.wait().unwrap().success());
}

#[test]
fn start_refuses_outside_a_repository() {
    let lab = Lab::new();
    let elsewhere = TempDir::new().unwrap();

    let output = saaio_command(&lab.home)
        .args(["start", "x"])
        .current_dir(elsewhere.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("needs a git repository"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn start_refuses_a_config_with_the_top_level_agent_key() {
    let lab = Lab::new();
    fs::create_dir_all(&lab.home).unwrap();
    fs::write(lab.home.join("no-config.toml"), "agent = \"codex\"\n").unwrap();

    let output = lab.start(&["SHOP-1 old config"]);

    assert_eq!(output.status.code(), Some(1));
    let said = format!(
        "{}{}",
        stdout(&output),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(said.contains("no-config.toml:1: "), "{said}");
    assert!(said.contains("`agent` under [worker]"), "{said}");
    assert_eq!(claimed_slots(&lab.home), [] as [String; 0]);
}

/// The slots claimed in the lab's shared folder.
fn claimed_slots(home: &Path) -> Vec<String> {
    fs::read_dir(shared(home).join("saaio/slots"))
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .filter(|name| name.parse::<u8>().is_ok())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn start_plain_opens_an_agent_in_any_folder_without_a_worktree_branch_or_slot() {
    let lab = Lab::new();
    let scratch = lab.dir.path().canonicalize().unwrap().join("scratch");
    fs::create_dir_all(&scratch).unwrap();

    let output = saaio_command(&lab.home)
        .args(["start", "--plain"])
        .arg(&scratch)
        .current_dir(&scratch)
        .output()
        .unwrap();

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.starts_with(
            "flow: scratch
"
        ),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "folder: {}
agent: claude
pane: when saaio opens",
            scratch.display()
        )),
        "{text}"
    );
    assert!(!text.contains("branch"), "{text}");
    assert!(!scratch.join(".saaio").exists(), "no worktree");
    assert!(claimed_slots(&lab.home).is_empty(), "no slot");

    let board: serde_json::Value =
        serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap();
    let session = &board["flows"][0];
    assert_eq!(session["mode"], "plain");
    assert_eq!(session["state"], "idle", "it waits at an empty prompt");
    assert_eq!(session["request"], "");
    assert_eq!(session["steps"], serde_json::json!([]));
    assert_eq!(session["workspace"]["path"], scratch.display().to_string());
    assert_eq!(session["workspace"]["branch"], serde_json::Value::Null);
    assert_eq!(session["workspace"]["slot"], serde_json::Value::Null);
}

#[test]
fn start_plain_takes_a_name_and_a_folder_under_home() {
    let lab = Lab::new();
    let home = lab.dir.path().canonicalize().unwrap();
    fs::create_dir_all(home.join("scratch")).unwrap();

    let output = saaio_command(&lab.home)
        .args(["start", "--plain", "~/scratch", "--name", "Try It"])
        .env("HOME", &home)
        .current_dir(&lab.repo)
        .output()
        .unwrap();

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.starts_with(
            "flow: try-it
"
        ),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "folder: {}
",
            home.join("scratch").display()
        )),
        "{text}"
    );
}

#[test]
fn start_plain_refuses_a_request_mode_or_type_and_a_missing_folder() {
    let lab = Lab::new();
    let scratch = lab.repo.display().to_string();

    for extra in [
        vec!["SHOP-1 x"],
        vec!["--mode", "auto"],
        vec!["--type", "feature"],
    ] {
        let mut args = vec!["--plain", &scratch];
        args.extend(extra.iter().copied());
        let output = lab.start(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{extra:?}: {}",
            stdout(&output)
        );
    }
    let plain_mode = lab.start(&["x", "--mode", "plain"]);
    assert_eq!(plain_mode.status.code(), Some(2), "plain is no --mode");

    let missing = lab.start(&["--plain", "nowhere"]);
    let text = stdout(&missing);
    assert_eq!(missing.status.code(), Some(1), "{text}");
    assert!(
        text.starts_with(&format!(
            "error: no folder {}",
            lab.repo.join("nowhere").display()
        )),
        "{text}"
    );
    assert!(text.contains("Name an existing folder"), "{text}");
    let file = lab.repo.join("file.txt");
    fs::write(&file, "x").unwrap();
    assert_eq!(lab.start(&["--plain", "file.txt"]).status.code(), Some(1));

    let board: serde_json::Value =
        serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap();
    assert_eq!(board["count"], 0, "nothing written");
}

#[test]
fn start_and_new_refuse_a_request_over_the_limit_before_making_anything() {
    let lab = Lab::new();
    let long = format!("SHOP-301 {}", "ä".repeat(20_000 - 9 + 1));
    let refused = |output: Output| {
        assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
        stdout(&output)
    };

    let text = refused(lab.start(&[&long]));
    assert!(
        text.contains("the request is 20001 chars, over 20000"),
        "{text}"
    );
    assert!(text.contains("name it in the request"), "{text}");
    let text = refused(lab.run(&["new", "--name", "shop-301", "--request", &long]));
    assert!(text.contains("over 20000"), "{text}");
    assert!(stdout(&lab.run(&["status"])).contains("count: 0"));
    assert_eq!(lab.branches(), [] as [String; 0]);

    let fits = &long[..long.len() - "ä".len()];
    assert!(lab.start(&[fits]).status.success());
    assert_eq!(lab.branches(), ["flow/shop-301"]);
}

#[test]
fn a_failing_setup_marks_the_flow_blocked_but_still_records_the_worker() {
    let lab = Lab::new();
    lab.hook(
        "setup",
        "#!/bin/sh\necho 'port 8001 is taken' >&2\nexit 1\n",
    );
    lab.trust();

    let output = lab.start(&["SHOP-218 fix totals"]);

    let text = stdout(&output);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("setup: \"failed: port 8001 is taken\""),
        "{text}"
    );
    assert!(text.contains("pane: when saaio opens"), "{text}");
    let board: serde_json::Value =
        serde_json::from_slice(&saaio(&lab.home, &["status", "--json"]).stdout).unwrap();
    assert_eq!(board["flows"][0]["state"], "blocked");
    assert_eq!(
        board["flows"][0]["blocker"],
        "setup failed: port 8001 is taken"
    );
}

#[test]
fn start_runs_nothing_of_a_repository_the_person_has_not_trusted() {
    let lab = Lab::new();
    lab.hook("setup", "#!/bin/sh\ntouch \"$SAAIO_REPO/setup-ran\"\n");
    lab.hook("teardown", "#!/bin/sh\nexit 0\n");

    let output = lab.start(&["SHOP-217 add CSV export"]);

    let text = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(
        text.contains(&format!(
            "{} is not trusted: saaio would run .saaio/setup and .saaio/teardown",
            lab.repo.display()
        )),
        "{text}"
    );
    assert!(text.contains("run `saaio trust`"), "{text}");
    assert!(!lab.repo.join("setup-ran").exists());
    assert!(!lab.repo.join(".saaio/worktrees").exists());
    assert_eq!(lab.branches(), [] as [String; 0]);
    assert!(stdout(&saaio(&lab.home, &["status"])).contains("count: 0"));

    lab.trust();
    assert!(lab.start(&["SHOP-217 add CSV export"]).status.success());
    assert!(lab.repo.join("setup-ran").exists());
}

#[test]
fn start_needs_no_trust_in_a_repository_without_scripts() {
    let lab = Lab::new();

    let output = lab.start(&["SHOP-217 add CSV export"]);

    assert!(output.status.success(), "{}", stdout(&output));
}

#[test]
fn start_refuses_a_worktrees_folder_that_links_elsewhere() {
    let lab = Lab::new();
    let elsewhere = TempDir::new().unwrap();
    fs::create_dir(lab.repo.join(".saaio")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), lab.repo.join(".saaio/worktrees")).unwrap();

    let output = lab.start(&["SHOP-217 add CSV export"]);

    let text = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(
        text.contains(&format!(
            "{} is a symbolic link",
            lab.repo.join(".saaio/worktrees").display()
        )),
        "{text}"
    );
    assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
    assert_eq!(lab.branches(), [] as [String; 0]);
    let board = stdout(&saaio(&lab.home, &["status"]));
    assert!(!board.contains("shop-217"), "{board}");
}

#[test]
fn start_says_when_every_slot_is_taken() {
    let lab = Lab::new();
    for slot in 1..=20 {
        seed(
            &lab.home,
            &format!("f{slot}"),
            &[
                &format!(
                    r#"{{"v":1,"at":1,"kind":"started","name":"f{slot}","request":"x","mode":"guided"}}"#
                ),
                &format!(
                    r#"{{"v":1,"at":2,"kind":"workspace","path":"/p{slot}","branch":"b","slot":{slot},"window":null,"agent":"claude"}}"#
                ),
            ],
        );
    }

    let output = lab.start(&["one more"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("all 20 slots are in use"),
        "{}",
        stdout(&output)
    );
    let logs = fs::read_dir(lab.home.join("flows"))
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().path().extension() == Some("jsonl".as_ref()))
        .count();
    assert_eq!(logs, 20, "the failed start forgets its flow");
}

#[test]
fn a_start_whose_worktree_fails_forgets_its_flow_and_gives_its_slot_back() {
    let lab = Lab::new();
    let worktrees = lab.repo.join(".saaio/worktrees");
    fs::create_dir_all(&worktrees).unwrap();
    fs::write(worktrees.join("shop-9"), "in the way").unwrap();

    let failed = lab.start(&["SHOP-9 export"]);

    assert_eq!(failed.status.code(), Some(1), "{}", stdout(&failed));
    let logs = fs::read_dir(lab.home.join("flows")).map_or(0, Iterator::count);
    assert_eq!(logs, 0, "the failed start forgets its flow");
    assert!(
        !shared(&lab.home).join("saaio/slots/1").exists(),
        "the failed start gives its slot back"
    );
}

#[test]
fn projects_never_share_a_slot_and_show_up_together() {
    let shop = Lab::new();
    let mut api = Lab::new();
    api.home = shop.dir.path().join("projects/api");
    let shared_with_shop = |lab: &Lab, args: &[&str]| {
        saaio_command(&lab.home)
            .args(args)
            .current_dir(&lab.repo)
            .env("XDG_STATE_HOME", shared(&shop.home))
            .output()
            .unwrap()
    };

    let first = shared_with_shop(&shop, &["start", "SHOP-1 one"]);
    let second = shared_with_shop(&api, &["start", "API-1 one"]);

    assert!(stdout(&first).contains("slot: 1"), "{}", stdout(&first));
    assert!(stdout(&second).contains("slot: 2"), "{}", stdout(&second));
    let all = stdout(&shared_with_shop(&shop, &["status", "--all"]));
    assert!(all.contains("count: 2"), "{all}");
    assert!(all.contains("  api,api-1,"), "{all}");
    let json: serde_json::Value =
        serde_json::from_slice(&shared_with_shop(&api, &["status", "--all", "--json"]).stdout)
            .unwrap();
    let names: Vec<&str> = json["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|project| project["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 2, "{json}");
    assert!(names.contains(&"api"), "{json}");
}

#[test]
fn a_closed_pr_keeps_an_ended_flow_finished_after_its_slot_is_reclaimed() {
    let lab = Lab::new();
    git(
        &lab.repo,
        &["remote", "add", "origin", "https://github.com/a/b.git"],
    );
    let first = lab.start(&["SHOP-1 first"]);
    assert!(first.status.success(), "{}", stdout(&first));
    let first_id = id_in(&stdout(&first));
    assert!(stdout(&first).contains("slot: 1"));

    let ended = saaio_in(&lab.home, &first_id, &["report", "ended"]);
    assert!(ended.status.success(), "{}", stdout(&ended));
    let opened = saaio_in(
        &lab.home,
        &first_id,
        &["report", "merge-request", "https://github.com/a/b/pull/45"],
    );
    assert!(opened.status.success(), "{}", stdout(&opened));

    let second = lab.start(&["SHOP-2 second"]);
    assert!(second.status.success(), "{}", stdout(&second));
    assert!(stdout(&second).contains("slot: 1"), "{}", stdout(&second));
    let second_id = id_in(&stdout(&second));

    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(lab.home.join("flows").join(format!("{first_id}.jsonl")))
        .unwrap();
    writeln!(
        log,
        r#"{{"v":1,"at":9999999999999,"kind":"merge-request-closed","url":"https://github.com/a/b/pull/45"}}"#
    )
    .unwrap();

    let board: serde_json::Value =
        serde_json::from_slice(&lab.run(&["status", "--json"]).stdout).unwrap();
    let flows = board["flows"].as_array().unwrap();
    let old = flows.iter().find(|flow| flow["id"] == first_id).unwrap();
    let new = flows.iter().find(|flow| flow["id"] == second_id).unwrap();
    assert_eq!(old["state"], "ended");
    assert!(old["mergeRequest"].is_null());
    assert_eq!(
        old["timeline"].as_array().unwrap().last().unwrap()["text"],
        "PR #45 closed"
    );
    assert_eq!(new["state"], "working");
    assert_eq!(new["workspace"]["slot"], 1);
    let claim: serde_json::Value =
        serde_json::from_slice(&fs::read(shared(&lab.home).join("saaio/slots/1")).unwrap())
            .unwrap();
    assert_eq!(claim["flow"], second_id);
}

#[test]
fn a_failed_start_gives_its_slot_back() {
    let lab = Lab::new();
    fs::create_dir_all(lab.repo.join(".saaio/worktrees/shop-1")).unwrap();
    fs::write(lab.repo.join(".saaio/worktrees/shop-1/x"), "in the way").unwrap();

    let failed = lab.start(&["SHOP-1 one"]);
    let next = lab.start(&["SHOP-2 two"]);

    assert_eq!(failed.status.code(), Some(1), "{}", stdout(&failed));
    assert!(stdout(&next).contains("slot: 1"), "{}", stdout(&next));
    let list = stdout(&lab.run(&["status"]));
    assert!(!list.contains("shop-1,"), "{list}");
}

#[test]
fn start_names_the_branch_after_the_repos_convention() {
    let lab = Lab::new();
    fs::create_dir_all(lab.repo.join(".saaio")).unwrap();
    fs::write(
        lab.repo.join(".saaio/config.toml"),
        "[branch]\nname = \"{type}/{ticket}-{slug}\"\ntypes = [\"feature\", \"bugfix\"]\n",
    )
    .unwrap();
    git(&lab.repo, &["branch", "feature/SHOP-2-export-orders"]);

    let bugfix = lab.start(&["SHOP-1 fix totals", "--type", "bugfix"]);
    let feature = lab.start(&["SHOP-2 export orders"]);
    let unknown = lab.start(&["SHOP-3 x", "--type", "hotfix"]);

    assert!(
        stdout(&bugfix).contains("branch: bugfix/SHOP-1-fix-totals"),
        "{}",
        stdout(&bugfix)
    );
    assert!(
        stdout(&feature).contains("branch: feature/SHOP-2-export-orders-2"),
        "a taken branch gets -2: {}",
        stdout(&feature)
    );
    assert!(
        lab.repo.join(".saaio/worktrees/shop-1").exists(),
        "worktrees keep the flow's name"
    );
    assert_eq!(unknown.status.code(), Some(1));
    assert!(
        stdout(&unknown).contains("no branch type hotfix; this repo has feature, bugfix"),
        "{}",
        stdout(&unknown)
    );
    assert!(
        !stdout(&lab.run(&[])).contains("shop-3"),
        "nothing was created"
    );
}

#[test]
fn start_refuses_a_type_in_a_repo_without_types() {
    let lab = Lab::new();

    let output = lab.start(&["SHOP-1 x", "--type", "bugfix"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("no .saaio/config.toml"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn setup_gets_no_key_of_the_session_that_started_the_flow() {
    let lab = Lab::new();
    lab.hook(
        "setup",
        "#!/bin/sh\nprintf '%s' \"${SAAIO_PROOF-none}\" > setup-key\n",
    );
    lab.trust();

    let output = saaio_command(&lab.home)
        .args(["start", "SHOP-217 add CSV export"])
        .current_dir(&lab.repo)
        .env("SAAIO_PROOF", "the-masters-key")
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", stderr(&output));
    let worktree = lab.repo.join(".saaio/worktrees/shop-217");
    assert_eq!(
        fs::read_to_string(worktree.join("setup-key")).unwrap(),
        "none"
    );
}
