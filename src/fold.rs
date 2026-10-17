use crate::event::{Event, EventKind, Mode, Phase};
use crate::store::FlowLog;
use crate::view::{
    Board, FlowView, ReviewCount, Session, State, Step, StepStatus, Tasks, TimelineEntry, Tone,
    VIEW_VERSION, Workspace,
};
use crate::workflow::Workflow;

/// A flow with a heartbeat older than this, not done or ended, is not responding.
pub const SILENT_AFTER_MS: u64 = 90_000;

/// One flow's view at `now`; None when the log has no Started event to name it.
pub fn fold<'a>(
    id: &str,
    events: impl IntoIterator<Item = &'a Event>,
    last_beat: Option<u64>,
    now: u64,
) -> Option<FlowView> {
    let mut events = events.into_iter();
    let (name, request, mode, phases, at) =
        events.by_ref().find_map(|event| match &event.kind {
            EventKind::Started {
                name,
                request,
                mode,
                phases,
            } => Some((name, request, *mode, phases.as_deref(), event.at)),
            _ => None,
        })?;
    let mut folding = Folding::new(id, name, request, mode, phases, at);
    for event in events {
        folding.apply(event);
    }
    let mut view = folding.finish(now);
    let is_silent = last_beat.is_some_and(|beat| now.saturating_sub(beat) > SILENT_AFTER_MS);
    if is_silent && !view.is_finished() {
        view.state = State::NotResponding;
    }
    Some(view)
}

/// True when appending `kind` changes what the flow shows: its state, phase, blocker, delegate,
/// tasks, review, merge request, decisions, answer or session; every message does. Adapters skip the rest,
/// so a log does not grow by a line for every tool call.
pub fn changes(id: &str, events: &[Event], kind: &EventKind, now: u64) -> bool {
    if matches!(kind, EventKind::Message { .. }) {
        return true;
    }
    let Some(before) = fold(id, events, None, now) else {
        return true;
    };
    let next = Event::new(now, kind.clone());
    let Some(after) = fold(id, events.iter().chain([&next]), None, now) else {
        return true;
    };
    shown(&before) != shown(&after)
}

/// What a flow shows that an event can change.
fn shown(view: &FlowView) -> impl PartialEq + '_ {
    (
        view.state,
        view.phase.as_ref(),
        view.blocker.as_deref(),
        view.delegate.as_deref(),
        view.tasks,
        view.review,
        view.merge_request.as_deref(),
        &view.decisions,
        view.timeline.last().map(|entry| entry.text.as_str()),
        view.answer.as_deref(),
        view.session.as_ref(),
    )
}

/// Every log that names its flow, folded at `now`.
pub fn views(logs: &[FlowLog], now: u64) -> Vec<FlowView> {
    logs.iter()
        .filter_map(|log| fold(&log.id, &log.events, log.beat, now))
        .collect()
}

/// Sorted: needs you, ready (idle), running, finished; newest change first within each group.
pub fn board(mut flows: Vec<FlowView>, now: u64) -> Board {
    flows.sort_by_key(|flow| (flow.group(), std::cmp::Reverse(flow.changed_at)));
    Board {
        version: VIEW_VERSION,
        now,
        count: flows.len(),
        needs_you: flows.iter().filter(|flow| flow.needs_you()).count(),
        flows,
    }
}

/// The flow a name or id means: an exact id, else the unfinished flow of that name,
/// else the most recent finished one of that name.
pub fn resolve<'a>(flows: &'a [FlowView], name_or_id: &str) -> Option<&'a FlowView> {
    let named = || flows.iter().filter(|flow| flow.name == name_or_id);
    flows
        .iter()
        .find(|flow| flow.id == name_or_id)
        .or_else(|| {
            named()
                .filter(|flow| !flow.is_finished())
                .max_by_key(|flow| flow.changed_at)
        })
        .or_else(|| named().max_by_key(|flow| flow.changed_at))
}

/// The state the last of `kinds` that says one puts the master in: Working, Delegated,
/// Waiting, Idle, Blocked or Ended; None when none says one (Answer, Session, Phase, ...).
pub fn master_state(kinds: &[EventKind]) -> Option<State> {
    kinds.iter().rev().find_map(|kind| match kind {
        EventKind::Working => Some(State::Working),
        EventKind::Delegated { .. } => Some(State::Delegated),
        EventKind::Waiting { .. } => Some(State::Waiting),
        EventKind::Idle { .. } => Some(State::Idle),
        EventKind::Blocked { .. } => Some(State::Blocked),
        EventKind::Ended => Some(State::Ended),
        _ => None,
    })
}

#[derive(Debug)]
struct Folding {
    view: FlowView,
    phases: Vec<Phase>,
    /// No list recorded: "design" reads as "plan".
    legacy: bool,
    /// By position in phases.
    durations: Vec<Option<u64>>,
    entered: Vec<bool>,
    phase_since: u64,
    stopped_at: Option<u64>,
}

impl Folding {
    fn new(
        id: &str,
        name: &str,
        request: &str,
        mode: Mode,
        phases: Option<&[Phase]>,
        at: u64,
    ) -> Folding {
        let view = FlowView {
            id: id.to_string(),
            name: name.to_string(),
            request: request.to_string(),
            mode,
            phase: None,
            // A plain session opens at an empty prompt; a flow's agent starts on its request.
            state: if mode == Mode::Plain {
                State::Idle
            } else {
                State::Working
            },
            blocker: None,
            delegate: None,
            tasks: None,
            review: None,
            merge_request: None,
            decisions: Vec::new(),
            steps: Vec::new(),
            timeline: vec![entry(at, "started".to_string(), Tone::Neutral)],
            started_at: at,
            changed_at: at,
            workspace: None,
            answer: None,
            session: None,
        };
        let legacy = phases.is_none();
        let phases = phases.map_or_else(|| Workflow::builtin().phases(), <[Phase]>::to_vec);
        Folding {
            view,
            legacy,
            durations: vec![None; phases.len()],
            entered: vec![false; phases.len()],
            phases,
            phase_since: at,
            stopped_at: None,
        }
    }

    fn apply(&mut self, event: &Event) {
        let before = (
            self.view.state,
            self.view.phase.clone(),
            self.view.blocker.clone(),
        );
        let at = event.at;
        match &event.kind {
            EventKind::Started { .. } => {}
            EventKind::Phase { phase } => {
                if let Some(position) = self.position(phase)
                    && self.view.phase.as_ref() != Some(&self.phases[position])
                {
                    self.enter(position, at);
                    // A delegate can start the phase itself, as a review in a fresh agent does.
                    if self.view.state != State::Delegated {
                        self.view.state = State::Working;
                        self.view.delegate = None;
                    }
                    self.view.blocker = None;
                    let text = format!("phase: {}", self.phases[position].name());
                    self.note(at, text, Tone::Progress);
                }
            }
            EventKind::Working => {
                if matches!(self.view.state, State::Waiting | State::Blocked) {
                    self.note(at, "answered".to_string(), Tone::Progress);
                }
                self.view.state = State::Working;
                self.view.blocker = None;
                self.view.delegate = None;
            }
            EventKind::Delegated { description } => {
                self.view.state = State::Delegated;
                self.view.delegate = Some(description.clone());
            }
            EventKind::Waiting { question } => {
                self.stop_for(State::Waiting, question, at, "waiting", Tone::Attention);
            }
            EventKind::Idle { note } => {
                let text = format!("idle: {note}");
                let same = self.view.state == State::Idle
                    && self.view.timeline.last().map(|entry| &entry.text) == Some(&text);
                if !same {
                    self.view.changed_at = at;
                    self.view.state = State::Idle;
                    self.view.blocker = None;
                    self.view.delegate = None;
                    self.note(at, text, Tone::Neutral);
                }
            }
            EventKind::Blocked { reason } => {
                self.stop_for(State::Blocked, reason, at, "blocked", Tone::Problem);
            }
            EventKind::Decision { text } => {
                if !self.view.decisions.contains(text) {
                    self.view.decisions.push(text.clone());
                    self.note(at, format!("decided: {text}"), Tone::Neutral);
                }
            }
            EventKind::Tasks { done, total } => {
                self.view.tasks = Some(Tasks {
                    done: *done,
                    total: *total,
                });
            }
            EventKind::Review { fixed, open } => {
                let review = ReviewCount {
                    fixed: *fixed,
                    open: *open,
                };
                self.view.review = Some(review);
                let tone = if *open > 0 {
                    Tone::Attention
                } else {
                    Tone::Progress
                };
                self.note(at, format!("review: {review}"), tone);
            }
            EventKind::MergeRequest { url } => {
                if let Some(last) = self.phases.len().checked_sub(1)
                    && self.view.phase.as_ref() != self.phases.last()
                {
                    self.enter(last, at);
                }
                self.view.state = State::Done;
                self.view.blocker = None;
                self.view.delegate = None;
                self.view.merge_request = Some(url.clone());
                self.stopped_at.get_or_insert(at);
                self.note(at, "MR opened".to_string(), Tone::Success);
            }
            EventKind::Workspace {
                path,
                branch,
                slot,
                window,
                agent,
            } => {
                self.view.workspace = Some(Workspace {
                    path: path.clone(),
                    branch: branch.clone(),
                    slot: *slot,
                    window: window.clone(),
                    agent: agent.clone(),
                });
            }
            EventKind::Message { text } => {
                self.note(at, format!("master: {text}"), Tone::Neutral);
            }
            EventKind::Answer { text } => {
                self.view.answer = Some(text.clone());
            }
            EventKind::Session { id, transcript } => {
                self.view.session = Some(Session {
                    id: id.clone(),
                    transcript: transcript.clone(),
                });
            }
            EventKind::Ended => {
                if self.view.state != State::Done {
                    self.view.state = State::Ended;
                }
                self.view.delegate = None;
                self.stopped_at.get_or_insert(at);
                self.note(at, "ended".to_string(), Tone::Neutral);
            }
        }
        if before
            != (
                self.view.state,
                self.view.phase.clone(),
                self.view.blocker.clone(),
            )
        {
            self.view.changed_at = at;
        }
    }

    /// Where `phase` stands in the flow's phases (after the legacy rename); None when it has none.
    fn position(&self, phase: &Phase) -> Option<usize> {
        let phase = if self.legacy {
            legacy_name(phase)
        } else {
            phase.clone()
        };
        self.phases.iter().position(|known| *known == phase)
    }

    fn enter(&mut self, position: usize, at: u64) {
        self.close_phase(at);
        self.view.phase = Some(self.phases[position].clone());
        self.entered[position] = true;
        self.phase_since = at;
    }

    fn close_phase(&mut self, until: u64) {
        if let Some(current) = self
            .view
            .phase
            .as_ref()
            .and_then(|phase| self.position(phase))
        {
            let spent = until.saturating_sub(self.phase_since);
            let total = &mut self.durations[current];
            *total = Some(total.unwrap_or(0) + spent);
        }
    }

    fn stop_for(&mut self, state: State, text: &str, at: u64, label: &str, tone: Tone) {
        if self.view.state == state && self.view.blocker.as_deref() == Some(text) {
            return;
        }
        self.view.state = state;
        self.view.blocker = Some(text.to_string());
        self.note(at, format!("{label}: {text}"), tone);
    }

    fn note(&mut self, at: u64, text: String, tone: Tone) {
        self.view.timeline.push(entry(at, text, tone));
    }

    fn finish(mut self, now: u64) -> FlowView {
        self.close_phase(self.stopped_at.unwrap_or(now));
        if self.view.is_plain() {
            return self.view;
        }
        let current = self
            .view
            .phase
            .as_ref()
            .and_then(|phase| self.position(phase));
        let is_done = self.view.state == State::Done;
        self.view.steps = self
            .phases
            .iter()
            .enumerate()
            .map(|(position, phase)| {
                let status = if is_done {
                    StepStatus::Done
                } else if current == Some(position) {
                    StepStatus::Current
                } else if self.entered[position]
                    || current.is_some_and(|current| position < current)
                {
                    StepStatus::Done
                } else {
                    StepStatus::Open
                };
                Step {
                    phase: phase.clone(),
                    status,
                    duration_ms: self.durations[position],
                }
            })
            .collect();
        self.view
    }
}

/// "design" -> "plan", for logs written before phases were configurable.
fn legacy_name(phase: &Phase) -> Phase {
    match phase.name() {
        "design" => "plan".parse().expect("a built-in phase name is valid"),
        _ => phase.clone(),
    }
}

fn entry(at: u64, text: String, tone: Tone) -> TimelineEntry {
    TimelineEntry { at, text, tone }
}

#[cfg(test)]
mod tests;
