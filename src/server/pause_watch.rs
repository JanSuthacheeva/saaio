use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

use super::{Event, Server, spawn_named};
use crate::event::{Event as LogEvent, EventKind, ServiceStatus};
use crate::launch::worktree;
use crate::pause::{self, Job};
use crate::server_log;
use crate::view::FlowView;
use crate::workspace::hooks::{self, Hook};

/// The longest a pause or resume script may run before it is killed.
pub const HOOK_LIMIT: Duration = Duration::from_mins(5);

/// What a job's thread sends back.
#[derive(Debug)]
pub struct Finished {
    pub id: String,
    pub job: Job,
    /// Ok(false): the script was gone by the time it ran.
    pub result: Result<bool, String>,
}

/// The flows a job runs for.
#[derive(Debug, Default)]
pub struct PauseWatch {
    running: HashSet<String>,
}

impl PauseWatch {
    /// The flows and jobs to start now, with none running for them; marks them running.
    pub fn due(
        &mut self,
        flows: &[FlowView],
        now: u64,
        after: Option<Duration>,
    ) -> Vec<(FlowView, Job)> {
        let mut due = Vec::new();
        for flow in flows {
            if self.running.contains(&flow.id) {
                continue;
            }
            if let Some(job) = pause::next(flow, now, after) {
                self.running.insert(flow.id.clone());
                due.push((flow.clone(), job));
            }
        }
        due
    }

    /// The job for `id` ended.
    pub fn done(&mut self, id: &str) {
        self.running.remove(id);
    }
}

impl Server {
    /// Each tick: starts the jobs due, for flows whose repo has both scripts; appends pausing or
    /// resuming before the thread starts.
    pub(super) fn pause_idle(&mut self) {
        let now = (self.clock)();
        let after = self.config.services.pause_after();
        for (flow, job) in self.pauses.due(&self.board.flows, now, after) {
            let Some(repo) = scripted_repo(&flow) else {
                self.pauses.done(&flow.id);
                continue;
            };
            let status = match job {
                Job::Pause => ServiceStatus::Pausing,
                Job::Resume => ServiceStatus::Resuming,
            };
            let kind = EventKind::Services {
                status,
                reason: None,
            };
            if let Err(error) = self.store.append(&flow.id, &LogEvent::new(now, kind)) {
                server_log::write(now, &format!("{error:#}"));
                self.pauses.done(&flow.id);
                continue;
            }
            run_in_background(flow, repo, job, self.events.clone());
        }
    }

    /// Appends paused, running, pause-failed or resume-failed; logs a failure.
    pub(super) fn pause_finished(&mut self, finished: Finished) {
        let now = (self.clock)();
        self.pauses.done(&finished.id);
        let (status, reason) = match (finished.job, finished.result) {
            (Job::Pause, Ok(true)) => (ServiceStatus::Paused, None),
            (Job::Resume, Ok(true)) | (_, Ok(false)) => (ServiceStatus::Running, None),
            (Job::Pause, Err(error)) => (ServiceStatus::PauseFailed, Some(error)),
            (Job::Resume, Err(error)) => (ServiceStatus::ResumeFailed, Some(error)),
        };
        if let Some(reason) = &reason {
            let name = self
                .board
                .flows
                .iter()
                .find(|flow| flow.id == finished.id)
                .map_or(finished.id.as_str(), |flow| flow.name.as_str());
            let job = match finished.job {
                Job::Pause => "pausing",
                Job::Resume => "resuming",
            };
            server_log::write(
                now,
                &format!("{job} the services of {name} failed: {reason}"),
            );
        }
        let kind = EventKind::Services { status, reason };
        if let Err(error) = self.store.append(&finished.id, &LogEvent::new(now, kind)) {
            server_log::write(now, &format!("{error:#}"));
        }
    }
}

/// The repository of a flow whose worktree is there and whose repo has both .saaio/pause and
/// .saaio/resume; None when either is missing.
fn scripted_repo(flow: &FlowView) -> Option<PathBuf> {
    let space = flow.workspace.as_ref()?;
    let repo = worktree::repo_of(space)?;
    let scripted = Path::new(&space.path).is_dir()
        && hooks::exists(&repo, Hook::Pause)
        && hooks::exists(&repo, Hook::Resume);
    scripted.then_some(repo)
}

/// The script on a thread of its own; Event::Paused(Finished) when it ends.
fn run_in_background(flow: FlowView, repo: PathBuf, job: Job, events: Sender<Event>) {
    spawn_named(format!("pause {}", flow.name), move || {
        let Some(space) = &flow.workspace else {
            return;
        };
        let hook = match job {
            Job::Pause => Hook::Pause,
            Job::Resume => Hook::Resume,
        };
        let env = worktree::hook_env(&flow.id, &flow.name, &repo, space);
        let result = hooks::run_within(&repo, hook, space, &env, HOOK_LIMIT)
            .map_err(|error| format!("{error:#}"));
        let _ = events.send(Event::Paused(Finished {
            id: flow.id,
            job,
            result,
        }));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Mode;
    use crate::fold::fold;

    const MIN: u64 = 60_000;
    const AFTER: Option<Duration> = Some(Duration::from_mins(5));

    fn idle(id: &str) -> FlowView {
        let events = [
            LogEvent::new(
                0,
                EventKind::Started {
                    name: id.into(),
                    request: "x".into(),
                    mode: Mode::Auto,
                    phases: None,
                },
            ),
            LogEvent::new(
                0,
                EventKind::Workspace {
                    path: format!("/repo/.saaio/worktrees/{id}"),
                    branch: Some(format!("flow/{id}")),
                    slot: Some(1),
                    window: None,
                    agent: "claude".into(),
                },
            ),
            LogEvent::new(
                0,
                EventKind::Idle {
                    note: "done".into(),
                },
            ),
        ];
        fold(id, &events, None, 0).unwrap()
    }

    #[test]
    fn runs_one_job_per_flow_until_it_is_done() {
        let flows = [idle("csv"), idle("pdf")];
        let mut watch = PauseWatch::default();

        let due: Vec<_> = watch
            .due(&flows, 10 * MIN, AFTER)
            .into_iter()
            .map(|(flow, job)| (flow.id, job))
            .collect();
        assert_eq!(
            due,
            vec![("csv".into(), Job::Pause), ("pdf".into(), Job::Pause)]
        );
        assert!(
            watch.due(&flows, 10 * MIN, AFTER).is_empty(),
            "still running"
        );

        watch.done("csv");
        let again = watch.due(&flows, 10 * MIN, AFTER);
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].0.id, "csv");
        assert!(watch.due(&flows, 10 * MIN, None).is_empty());
    }
}
