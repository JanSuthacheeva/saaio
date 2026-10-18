use ratatui::style::Style;
use ratatui::text::Span;

use super::Sidebar;
use crate::proto::PaneStatus;
use crate::text::merge_request;
use crate::view::{FlowView, State, StepStatus};

impl Sidebar<'_> {
    pub(super) fn spinner(&self) -> &'static str {
        let spinner = self.theme.glyphs.spinner;
        spinner[self.spin % spinner.len()]
    }

    pub fn glyph(&self, state: State) -> &'static str {
        let glyphs = &self.theme.glyphs;
        match state {
            State::Working | State::Delegated => self.spinner(),
            State::Waiting => glyphs.waiting,
            State::Idle => glyphs.idle,
            State::Blocked | State::NotResponding => glyphs.blocked,
            State::Done => glyphs.done,
            State::Ended => glyphs.ended,
        }
    }

    /// ●───●───○───○: done steps and the joins they reached in text, the current one in its phase's
    /// color as phase_colors says (step_style), the rest faint; all of it in text once finished.
    pub fn steps(&self, flow: &FlowView) -> Vec<Span<'static>> {
        let theme = self.theme;
        let glyphs = &theme.glyphs;
        let mut spans = Vec::new();
        for (index, step) in flow.steps.iter().enumerate() {
            let (glyph, style) = match step.status {
                _ if flow.is_finished() => (glyphs.step_done, theme.ui.text),
                StepStatus::Done => (glyphs.step_done, theme.ui.text),
                StepStatus::Current => (glyphs.step_current, theme.step_style(flow)),
                StepStatus::Open => (glyphs.step_open, theme.ui.faint),
            };
            spans.push(Span::styled(glyph, style));
            if let Some(next) = flow.steps.get(index + 1) {
                let reached = step.status == StepStatus::Done && next.status != StepStatus::Open;
                let join = if flow.is_finished() || reached {
                    theme.ui.text
                } else {
                    theme.ui.faint
                };
                spans.push(Span::styled(glyphs.step_join, join));
            }
        }
        spans
    }

    pub fn label(&self, flow: &FlowView) -> Span<'static> {
        let theme = self.theme;
        match (flow.state, flow.merge_request.as_deref()) {
            (State::Done | State::Ended, Some(url)) => {
                Span::styled(merge_request(url), theme.ui.faint)
            }
            _ => self.state_label(flow),
        }
    }

    fn state_label(&self, flow: &FlowView) -> Span<'static> {
        let theme = self.theme;
        match flow.state {
            State::Done => Span::styled("done", theme.state.done),
            State::Ended => Span::styled("ended", theme.state.ended),
            State::NotResponding => Span::styled("not responding", theme.state.blocked),
            _ => match (&flow.phase, flow.state) {
                (Some(phase), _) => Span::styled(phase.name().to_string(), theme.ui.text),
                (None, State::Working) => Span::styled("starting", theme.ui.faint),
                (None, State::Waiting) => Span::styled("waiting", theme.state.waiting),
                (None, State::Idle) => Span::styled("idle", theme.ui.muted),
                (None, State::Delegated) => Span::styled("delegated", theme.state.working),
                (None, _) => Span::styled("blocked", theme.state.blocked),
            },
        }
    }

    /// The third row: why the pane is not running, what the flow asks, or what it does.
    pub(super) fn note(
        &self,
        flow: &FlowView,
        status: Option<&PaneStatus>,
    ) -> Option<(String, Style)> {
        let theme = self.theme;
        if let Some(PaneStatus::Failed { reason }) = status {
            return Some((reason.clone(), theme.state.blocked));
        }
        if flow.is_finished() {
            return None;
        }
        if let Some(blocker) = &flow.blocker {
            return Some((blocker.clone(), theme.ui.faint));
        }
        if let Some(delegate) = &flow.delegate {
            return Some((format!("agent: {delegate}"), theme.ui.faint));
        }
        last_note(flow).map(|note| (note, theme.ui.faint))
    }
}

/// What a flow last said that its row does not already show: its last timeline entry, without
/// "started", phase changes and "answered" (the steps and the glyph say those), and without the
/// "idle: " the glyph says too.
pub fn last_note(flow: &FlowView) -> Option<String> {
    let text = flow
        .timeline
        .iter()
        .skip(1)
        .rev()
        .map(|entry| entry.text.as_str())
        .find(|text| !text.starts_with("phase: "))?;
    // An answered question is over; what came before it is no news.
    (text != "answered").then(|| text.strip_prefix("idle: ").unwrap_or(text).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::prefix::Prefix;

    #[test]
    fn colors_only_the_current_step() {
        use crate::event::{Event, EventKind, Mode, Phase};
        use crate::proto::PaneKey;
        use crate::ui::theme::{Palette, Theme};
        use crate::view::{Board, VIEW_VERSION};
        let events = [
            EventKind::Started {
                name: "shop".into(),
                request: "x".into(),
                mode: Mode::Auto,
                phases: None,
            },
            EventKind::Phase {
                phase: Phase::of("plan"),
            },
            EventKind::Phase {
                phase: Phase::of("implement"),
            },
        ]
        .map(|kind| Event::new(0, kind));
        let flow = crate::fold::fold("id", &events, None, 0).unwrap();
        let theme = Theme::from_palette(Palette::SAAIO);
        let board = Board {
            version: VIEW_VERSION,
            now: 0,
            count: 0,
            needs_you: 0,
            flows: Vec::new(),
        };
        let sidebar = Sidebar {
            board: &board,
            panes: &[],
            focus: &PaneKey::Master,
            cursor: None,
            peek: &[],
            master: None,
            master_state: None,
            now: 0,
            spin: 0,
            changed: &std::collections::HashMap::new(),
            home: None,
            prefix: Prefix::default(),
            theme: &theme,
        };

        let styles: Vec<_> = sidebar.steps(&flow).iter().map(|span| span.style).collect();

        assert_eq!(
            styles,
            [
                theme.ui.text,
                theme.ui.text,
                theme.phase.positions[1],
                theme.ui.faint,
                theme.ui.faint,
                theme.ui.faint,
                theme.ui.faint,
            ],
            "plan done, its join reached, implement current, the rest open"
        );
    }

    #[test]
    fn a_note_is_what_the_flow_last_said_not_what_the_row_shows() {
        use crate::event::{Event, EventKind, Mode, Phase};
        let flow = |kinds: Vec<EventKind>| {
            let mut events = vec![Event::new(
                0,
                EventKind::Started {
                    name: "shop".into(),
                    request: "x".into(),
                    mode: Mode::Auto,
                    phases: None,
                },
            )];
            events.extend(kinds.into_iter().map(|kind| Event::new(1, kind)));
            crate::fold::fold("id", &events, None, 2).unwrap()
        };
        let phase = |phase| EventKind::Phase { phase };

        let ready = flow(vec![
            phase(Phase::of("plan")),
            EventKind::Idle {
                note: "The design is ready for review.".into(),
            },
        ]);
        let working = flow(vec![
            phase(Phase::of("plan")),
            phase(Phase::of("implement")),
        ]);
        let told = flow(vec![
            EventKind::Message {
                text: "Semicolons.".into(),
            },
            EventKind::Working,
            phase(Phase::of("implement")),
        ]);

        assert_eq!(
            last_note(&ready).as_deref(),
            Some("The design is ready for review.")
        );
        assert_eq!(last_note(&working), None);
        assert_eq!(last_note(&told).as_deref(), Some("master: Semicolons."));
        let answered = flow(vec![
            EventKind::Waiting {
                question: "Which?".into(),
            },
            EventKind::Working,
        ]);
        assert_eq!(last_note(&answered), None, "not the old question");
    }
}
