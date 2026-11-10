//! `watch`: what changed since the last look. Pure functions — the loop, the state file and
//! the notifications live in the CLI.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::report::Report;
use crate::verdict::Verdict;

/// What `watch` remembers about one instance between runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Seen {
    /// Console name, or the end of the OCID.
    pub name: String,
    /// Shape.
    pub shape: String,
    /// `RUNNING`, `STOPPED`, ...
    pub state: String,
    /// The idle verdict.
    pub verdict: Verdict,
}

/// Every instance of the home region, by OCID.
pub type State = BTreeMap<String, Seen>;

/// The state a report describes.
pub fn state_of(report: &Report) -> State {
    report
        .instances
        .iter()
        .map(|r| {
            let seen = Seen {
                name: r.name.clone(),
                shape: r.shape.clone(),
                state: r.state.clone(),
                verdict: r.verdict,
            };
            (r.id.clone(), seen)
        })
        .collect()
}

/// Something worth telling the user about.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    /// The instance is no longer there: terminated, reclaimed, or deleted.
    Gone {
        /// Its name.
        name: String,
        /// Its shape.
        shape: String,
    },
    /// It was running and now is not.
    Stopped {
        /// Its name.
        name: String,
        /// The new lifecycle state, e.g. `STOPPED`.
        state: String,
    },
    /// It was not running and now is.
    Started {
        /// Its name.
        name: String,
    },
    /// The idle verdict changed while it was running.
    Verdict {
        /// Its name.
        name: String,
        /// Before.
        from: Verdict,
        /// Now.
        to: Verdict,
    },
    /// A new instance appeared.
    New {
        /// Its name.
        name: String,
        /// Its shape.
        shape: String,
    },
}

impl Event {
    /// Worth a notification, not just a line in the log.
    pub fn notify(&self) -> bool {
        !matches!(self, Event::New { .. })
    }

    /// Needs action now: the instance is gone or stopped, or at risk of reclamation.
    pub fn urgent(&self) -> bool {
        matches!(
            self,
            Event::Gone { .. }
                | Event::Stopped { .. }
                | Event::Verdict {
                    to: Verdict::AtRisk,
                    ..
                }
        )
    }

    /// One line for the log and the notification.
    pub fn message(&self) -> String {
        match self {
            Event::Gone { name, shape } => {
                format!("{name} ({shape}) is gone — terminated or reclaimed")
            }
            Event::Stopped { name, state } => {
                format!("{name} is {} — it was running", state.to_lowercase())
            }
            Event::Started { name } => format!("{name} is running again"),
            Event::Verdict { name, from, to } => {
                let why = match to {
                    Verdict::AtRisk => " — all metrics under 20%, idle by Oracle's rule",
                    Verdict::Close => " — close to the 20% line",
                    _ => "",
                };
                format!("{name}: {} → {}{why}", from.label(), to.label())
            }
            Event::New { name, shape } => format!("{name} ({shape}) is new — now watched"),
        }
    }
}

fn running(state: &str) -> bool {
    matches!(state, "RUNNING" | "STARTING" | "PROVISIONING")
}

/// Freeform tag that keeps `--auto-start` away from an instance: `oci-free-keeper: no-auto-start`.
pub const NO_AUTO_START: (&str, &str) = ("oci-free-keeper", "no-auto-start");

/// `--auto-start`: instances to start now — an Always Free shape, `STOPPED` now, and running or
/// stopping at the last look. One stopped before `watch` first looked is left alone, and so is
/// one in `opted_out` (by OCID). A start that fails is not retried: the next look sees it was
/// already `STOPPED`.
pub fn to_start<'a>(before: Option<&State>, now: &'a State, opted_out: &[&str]) -> Vec<&'a str> {
    let Some(before) = before else {
        return Vec::new();
    };
    now.iter()
        .filter(|(id, is)| {
            is.state == "STOPPED"
                && crate::rules::is_free_shape(&is.shape)
                && !opted_out.contains(&id.as_str())
                && before
                    .get(*id)
                    .is_some_and(|was| running(&was.state) || was.state == "STOPPING")
        })
        .map(|(id, _)| id.as_str())
        .collect()
}

/// Verdicts that describe a running instance's load; changes between others are noise.
fn load_verdict(v: Verdict) -> bool {
    matches!(v, Verdict::Safe | Verdict::Close | Verdict::AtRisk)
}

/// What changed from `before` to `now`. With no `before` (first run) nothing is an event.
pub fn diff(before: Option<&State>, now: &State) -> Vec<Event> {
    let Some(before) = before else {
        return Vec::new();
    };
    let mut events = Vec::new();
    for (id, was) in before {
        if !now.contains_key(id) {
            events.push(Event::Gone {
                name: was.name.clone(),
                shape: was.shape.clone(),
            });
        }
    }
    for (id, is) in now {
        let Some(was) = before.get(id) else {
            events.push(Event::New {
                name: is.name.clone(),
                shape: is.shape.clone(),
            });
            continue;
        };
        match (running(&was.state), running(&is.state)) {
            (true, false) => events.push(Event::Stopped {
                name: is.name.clone(),
                state: is.state.clone(),
            }),
            (false, true) => events.push(Event::Started {
                name: is.name.clone(),
            }),
            (true, true)
                if was.verdict != is.verdict
                    && load_verdict(was.verdict)
                    && load_verdict(is.verdict) =>
            {
                events.push(Event::Verdict {
                    name: is.name.clone(),
                    from: was.verdict,
                    to: is.verdict,
                });
            }
            _ => {}
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(name: &str, state: &str, verdict: Verdict) -> Seen {
        Seen {
            name: name.into(),
            shape: "VM.Standard.A1.Flex".into(),
            state: state.into(),
            verdict,
        }
    }

    fn st(items: &[(&str, Seen)]) -> State {
        items
            .iter()
            .map(|(id, s)| (id.to_string(), s.clone()))
            .collect()
    }

    #[test]
    fn first_run_is_quiet() {
        let now = st(&[("a", seen("arm", "STOPPED", Verdict::Stopped))]);
        assert!(diff(None, &now).is_empty());
    }

    #[test]
    fn nothing_changed_nothing_said() {
        let s = st(&[("a", seen("arm", "RUNNING", Verdict::Safe))]);
        assert!(diff(Some(&s), &s).is_empty());
    }

    #[test]
    fn gone_stopped_started() {
        let before = st(&[
            ("a", seen("arm", "RUNNING", Verdict::Safe)),
            ("b", seen("micro", "RUNNING", Verdict::AtRisk)),
            ("c", seen("spare", "STOPPED", Verdict::Stopped)),
        ]);
        let now = st(&[
            ("b", seen("micro", "STOPPED", Verdict::Stopped)),
            ("c", seen("spare", "RUNNING", Verdict::Safe)),
        ]);
        let events = diff(Some(&before), &now);
        assert_eq!(
            events,
            vec![
                Event::Gone {
                    name: "arm".into(),
                    shape: "VM.Standard.A1.Flex".into()
                },
                Event::Stopped {
                    name: "micro".into(),
                    state: "STOPPED".into()
                },
                Event::Started {
                    name: "spare".into()
                },
            ]
        );
        assert!(events[0].urgent() && events[1].urgent() && !events[2].urgent());
        assert_eq!(
            events[0].message(),
            "arm (VM.Standard.A1.Flex) is gone — terminated or reclaimed"
        );
        assert_eq!(events[1].message(), "micro is stopped — it was running");
    }

    #[test]
    fn verdict_changes_only_between_load_verdicts() {
        let before = st(&[
            ("a", seen("arm", "RUNNING", Verdict::Safe)),
            ("b", seen("micro", "RUNNING", Verdict::Unknown)),
        ]);
        let now = st(&[
            ("a", seen("arm", "RUNNING", Verdict::AtRisk)),
            ("b", seen("micro", "RUNNING", Verdict::Safe)),
        ]);
        let events = diff(Some(&before), &now);
        assert_eq!(
            events,
            vec![Event::Verdict {
                name: "arm".into(),
                from: Verdict::Safe,
                to: Verdict::AtRisk
            }]
        );
        assert!(events[0].urgent());
        assert!(events[0].message().contains("Safe → At risk"));
    }

    #[test]
    fn new_instances_are_logged_not_notified() {
        let before = st(&[]);
        let now = st(&[("a", seen("arm", "RUNNING", Verdict::Safe))]);
        let events = diff(Some(&before), &now);
        assert!(matches!(events[0], Event::New { .. }));
        assert!(!events[0].notify());
    }

    #[test]
    fn auto_start_only_what_stopped_while_watched() {
        let micro = |state: &str| Seen {
            shape: "VM.Standard.E2.1.Micro".into(),
            ..seen("micro", state, Verdict::Stopped)
        };
        let paid = |state: &str| Seen {
            shape: "VM.Standard.E4.Flex".into(),
            ..seen("big", state, Verdict::Stopped)
        };
        let before = st(&[
            ("a", seen("arm", "RUNNING", Verdict::Safe)),
            ("b", micro("STOPPING")),
            ("c", seen("spare", "STOPPED", Verdict::Stopped)),
            ("d", paid("RUNNING")),
            ("e", seen("mine", "RUNNING", Verdict::Safe)),
        ]);
        let now = st(&[
            ("a", seen("arm", "STOPPED", Verdict::Stopped)),
            ("b", micro("STOPPED")),
            ("c", seen("spare", "STOPPED", Verdict::Stopped)),
            ("d", paid("STOPPED")),
            ("e", seen("mine", "STOPPED", Verdict::Stopped)),
        ]);
        // spare was stopped already, big is not Always Free, mine is opted out.
        assert_eq!(to_start(Some(&before), &now, &["e"]), vec!["a", "b"]);
        assert!(to_start(None, &now, &[]).is_empty());
    }

    #[test]
    fn state_round_trips_through_json() {
        let s = st(&[("a", seen("arm", "RUNNING", Verdict::AtRisk))]);
        let text = serde_json::to_string(&s).unwrap();
        assert!(text.contains("\"at-risk\""));
        assert_eq!(serde_json::from_str::<State>(&text).unwrap(), s);
    }
}
