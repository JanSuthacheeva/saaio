use std::fmt;

use serde::{Deserialize, Serialize};

use crate::event::{EventKind, Mode, Phase};

pub const VIEW_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub version: u32,
    pub now: u64,
    pub count: usize,
    pub needs_you: usize,
    pub flows: Vec<FlowView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowView {
    pub id: String,
    pub name: String,
    pub request: String,
    pub mode: Mode,
    pub phase: Option<Phase>,
    pub state: State,
    pub blocker: Option<String>,
    pub delegate: Option<String>,
    pub tasks: Option<Tasks>,
    pub review: Option<ReviewCount>,
    pub merge_request: Option<String>,
    pub decisions: Vec<String>,
    pub steps: Vec<Step>,
    pub timeline: Vec<TimelineEntry>,
    pub started_at: u64,
    pub changed_at: u64,
    pub workspace: Option<Workspace>,
    /// The last turn's whole closing message.
    pub answer: Option<String>,
    /// The harness's own session, when it picks its id itself.
    pub session: Option<Session>,
}

/// A harness session: the id it resumes by and the transcript it writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub transcript: Option<String>,
}

/// Where a started flow's worker runs: its worktree, branch, port slot, window (flow-ui once
/// flow ui opened it; a tmux window id from before) and agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub path: String,
    pub branch: Option<String>,
    pub slot: Option<u8>,
    pub window: Option<String>,
    pub agent: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Working,
    Delegated,
    Waiting,
    /// The turn ended without a question: nothing to do for the person.
    Idle,
    Blocked,
    NotResponding,
    Done,
    Ended,
}

impl State {
    /// The name the JSON and TOON output use: working, not-responding, ...
    pub fn name(self) -> &'static str {
        match self {
            State::Working => "working",
            State::Delegated => "delegated",
            State::Waiting => "waiting",
            State::Idle => "idle",
            State::Blocked => "blocked",
            State::NotResponding => "not-responding",
            State::Done => "done",
            State::Ended => "ended",
        }
    }
}

/// Where the master's turn stands, as its hooks last said, and since when.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasterTurn {
    pub state: State,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub phase: Phase,
    pub status: StepStatus,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepStatus {
    Done,
    Current,
    Open,
}

impl StepStatus {
    /// The name the JSON and TOON output use.
    pub fn name(self) -> &'static str {
        match self {
            StepStatus::Done => "done",
            StepStatus::Current => "current",
            StepStatus::Open => "open",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tasks {
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewCount {
    pub fixed: u32,
    pub open: u32,
}

/// What `flow start` and `flow server` append when a flow gets or reopens its workspace.
impl From<Workspace> for EventKind {
    fn from(space: Workspace) -> EventKind {
        EventKind::Workspace {
            path: space.path,
            branch: space.branch,
            slot: space.slot,
            window: space.window,
            agent: space.agent,
        }
    }
}

/// 4/9
impl fmt::Display for Tasks {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}/{}", self.done, self.total)
    }
}

/// 2 fixed, 1 open
impl fmt::Display for ReviewCount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} fixed, {} open", self.fixed, self.open)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub at: u64,
    pub text: String,
    pub tone: Tone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tone {
    Neutral,
    Progress,
    Attention,
    Problem,
    Success,
}

impl Board {
    /// Pretty JSON with camelCase keys, `version` first, nothing truncated.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a board always serialises")
    }
}

/// What `flow status --all --json` prints: a board per project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Projects {
    pub version: u32,
    pub now: u64,
    pub projects: Vec<Project>,
}

/// One registered store: its project name, its folder and its flows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub name: String,
    pub root: String,
    pub board: Board,
}

impl Projects {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("projects always serialise")
    }
}

impl FlowView {
    /// Waiting, blocked or not responding.
    pub fn needs_you(&self) -> bool {
        matches!(
            self.state,
            State::Waiting | State::Blocked | State::NotResponding
        )
    }

    /// Done or ended.
    pub fn is_finished(&self) -> bool {
        matches!(self.state, State::Done | State::Ended)
    }

    /// Started with --plain: no phases, no master.
    pub fn is_plain(&self) -> bool {
        self.mode == Mode::Plain
    }

    pub fn group(&self) -> Group {
        if self.needs_you() {
            Group::NeedsYou
        } else if self.state == State::Idle {
            Group::Ready
        } else if self.is_finished() {
            Group::Done
        } else {
            Group::Running
        }
    }
}

/// What a flow needs, in the order the board and the sidebar list flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    NeedsYou,
    Ready,
    Running,
    Done,
}

impl Group {
    /// The sidebar's heading for the group.
    pub fn label(self) -> &'static str {
        match self {
            Group::NeedsYou => "NEEDS YOU",
            Group::Ready => "READY",
            Group::Running => "RUNNING",
            Group::Done => "DONE",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_states_and_steps_as_the_json_does() {
        let states = [
            State::Working,
            State::Delegated,
            State::Waiting,
            State::Idle,
            State::Blocked,
            State::NotResponding,
            State::Done,
            State::Ended,
        ];
        for state in states {
            assert_eq!(serde_json::to_value(state).unwrap(), state.name());
        }
        for status in [StepStatus::Done, StepStatus::Current, StepStatus::Open] {
            assert_eq!(serde_json::to_value(status).unwrap(), status.name());
        }
    }

    fn flow(state: State) -> FlowView {
        FlowView {
            id: "7f3c".into(),
            name: "shop-221".into(),
            request: "SHOP-221 add CSV export".into(),
            mode: Mode::Auto,
            phase: Some(Phase::of("merge-request")),
            state,
            blocker: None,
            delegate: None,
            tasks: Some(Tasks { done: 4, total: 9 }),
            review: None,
            merge_request: None,
            decisions: vec![],
            steps: vec![Step {
                phase: Phase::of("design"),
                status: StepStatus::Done,
                duration_ms: Some(540_000),
            }],
            timeline: vec![TimelineEntry {
                at: 1,
                text: "started".into(),
                tone: Tone::Neutral,
            }],
            started_at: 1,
            changed_at: 2,
            workspace: Some(Workspace {
                path: "/repo/.flow/worktrees/shop-221".into(),
                branch: Some("flow/shop-221".into()),
                slot: Some(1),
                window: Some("@14".into()),
                agent: "claude".into(),
            }),
            answer: None,
            session: None,
        }
    }

    #[test]
    fn serialises_the_board_with_version_first_and_camel_case_keys() {
        let board = Board {
            version: VIEW_VERSION,
            now: 9,
            count: 1,
            needs_you: 1,
            flows: vec![flow(State::NotResponding)],
        };

        let json = board.to_json();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(
            json.trim_start().starts_with("{\n  \"version\": 2,"),
            "{json}"
        );
        assert_eq!(value["needsYou"], 1);
        let flow = &value["flows"][0];
        assert_eq!(flow["state"], "not-responding");
        assert_eq!(flow["phase"], "merge-request");
        assert_eq!(flow["mergeRequest"], serde_json::Value::Null);
        assert_eq!(flow["startedAt"], 1);
        assert_eq!(flow["tasks"]["total"], 9);
        assert_eq!(flow["steps"][0]["durationMs"], 540_000);
        assert_eq!(flow["steps"][0]["status"], "done");
        assert_eq!(flow["timeline"][0]["tone"], "neutral");
        assert_eq!(flow["workspace"]["branch"], "flow/shop-221");
        assert_eq!(flow["workspace"]["window"], "@14");
    }

    #[test]
    fn needs_you_while_waiting_blocked_or_silent() {
        let needing: Vec<State> = [
            State::Working,
            State::Delegated,
            State::Waiting,
            State::Blocked,
            State::NotResponding,
            State::Done,
            State::Ended,
        ]
        .into_iter()
        .filter(|state| flow(*state).needs_you())
        .collect();

        assert_eq!(
            needing,
            vec![State::Waiting, State::Blocked, State::NotResponding]
        );
    }

    #[test]
    fn is_finished_when_done_or_ended() {
        assert!(flow(State::Done).is_finished());
        assert!(flow(State::Ended).is_finished());
        assert!(!flow(State::Blocked).is_finished());
        assert!(!flow(State::Working).is_finished());
    }

    #[test]
    fn is_plain_only_in_plain_mode() {
        let mut plain = flow(State::Working);
        assert!(!plain.is_plain());
        plain.mode = Mode::Plain;
        assert!(plain.is_plain());
    }
}
