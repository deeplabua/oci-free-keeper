//! The `check` report.

use serde::Serialize;

use crate::api::Snapshot;
use crate::model::Usage;
use crate::rules;
use crate::verdict::{Verdict, assess};

/// One instance in the report.
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    /// OCID — kept out of the JSON report, used by `watch` to follow an instance.
    #[serde(skip)]
    pub id: String,
    /// Console name, or the end of the OCID.
    pub name: String,
    /// Shape.
    pub shape: String,
    /// `RUNNING`, `STOPPED`, ...
    pub state: String,
    /// Utilisation, if read.
    pub usage: Option<Usage>,
    /// The verdict.
    pub verdict: Verdict,
    /// The busiest metric that counts, and its value, percent.
    pub busiest: Option<(&'static str, f64)>,
    /// Why there is no data, if so.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The whole report.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// Home region.
    pub home_region: String,
    /// `PAYG`, ... if known.
    pub payment_model: Option<String>,
    /// Window, days.
    pub window_days: u64,
    /// Warning threshold, percent.
    pub warn_percent: f64,
    /// When the rule was last checked.
    pub rule_as_of: &'static str,
    /// The instances.
    pub instances: Vec<Row>,
}

impl Report {
    /// `0` all safe, `1` close or unknown, `2` at risk or stopped.
    pub fn exit_code(&self) -> u8 {
        self.instances
            .iter()
            .map(|r| r.verdict.severity())
            .max()
            .unwrap_or(0)
    }

    /// Reclamation has not been seen on pay-as-you-go accounts (PRD, question 2).
    pub fn is_payg(&self) -> bool {
        self.payment_model.as_deref() == Some("PAYG")
    }
}

/// Judge every instance in the snapshot.
pub fn check(s: &Snapshot, warn_percent: f64) -> Report {
    let instances = s
        .instances
        .iter()
        .map(|o| {
            let i = &o.instance;
            let (usage, note) = match &o.usage {
                Ok(u) if u.cpu_p95.is_some() => (Some(u.clone()), None),
                Ok(_) => (None, None),
                Err(e) => (None, Some(e.clone())),
            };
            let a = assess(&i.shape, &i.lifecycle_state, usage.as_ref(), warn_percent);
            Row {
                id: i.id.clone(),
                name: i.name(),
                shape: i.shape.clone(),
                state: i.lifecycle_state.clone(),
                usage,
                verdict: a.verdict,
                busiest: a.busiest,
                note,
            }
        })
        .collect();
    Report {
        home_region: s.home_region.clone(),
        payment_model: s.payment_model.clone(),
        window_days: rules::WINDOW_DAYS,
        warn_percent,
        rule_as_of: rules::CHECKED,
        instances,
    }
}
