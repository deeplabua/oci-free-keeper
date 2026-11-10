//! How close an instance is to Oracle's idle rule.

use serde::Serialize;

use crate::model::Usage;
use crate::rules;

/// The verdict for one instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// At least one metric above the warning threshold.
    Safe,
    /// Every metric under the warning threshold, at least one at or above 20%.
    Close,
    /// Every metric under 20%: idle by Oracle's rule.
    AtRisk,
    /// No metric data.
    Unknown,
    /// Not an Always Free shape: the rule does not apply.
    NotSubject,
    /// Not running.
    Stopped,
}

impl Verdict {
    /// `✓`, `⚠`, `✗`, `?`, `—`.
    pub fn symbol(self) -> &'static str {
        match self {
            Verdict::Safe => "✓",
            Verdict::Close => "⚠",
            Verdict::AtRisk | Verdict::Stopped => "✗",
            Verdict::Unknown => "?",
            Verdict::NotSubject => "—",
        }
    }

    /// `Safe`, `At risk`, ...
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Safe => "Safe",
            Verdict::Close => "Close",
            Verdict::AtRisk => "At risk",
            Verdict::Unknown => "Unknown",
            Verdict::NotSubject => "Not subject",
            Verdict::Stopped => "Stopped",
        }
    }

    /// Exit-code weight: 0 fine, 1 look at it, 2 act now.
    pub fn severity(self) -> u8 {
        match self {
            Verdict::Safe | Verdict::NotSubject => 0,
            Verdict::Close | Verdict::Unknown => 1,
            Verdict::AtRisk | Verdict::Stopped => 2,
        }
    }
}

/// A verdict and the metric that decides it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Assessment {
    /// The verdict.
    pub verdict: Verdict,
    /// The busiest metric that counts (`cpu`, `memory`, `network`) and its value, percent.
    pub busiest: Option<(&'static str, f64)>,
}

/// Judge one instance.
pub fn assess(shape: &str, state: &str, usage: Option<&Usage>, warn_percent: f64) -> Assessment {
    let none = |verdict| Assessment {
        verdict,
        busiest: None,
    };
    if !rules::is_free_shape(shape) {
        return none(Verdict::NotSubject);
    }
    if !matches!(state, "RUNNING" | "STARTING" | "PROVISIONING") {
        return none(Verdict::Stopped);
    }
    let Some(u) = usage else {
        return none(Verdict::Unknown);
    };
    let mut metrics: Vec<(&'static str, f64)> = Vec::new();
    if let Some(v) = u.cpu_p95 {
        metrics.push(("cpu", v));
    }
    if let (true, Some(v)) = (rules::memory_counts(shape), u.memory_mean) {
        metrics.push(("memory", v));
    }
    if let Some(v) = u.network_percent {
        metrics.push(("network", v));
    }
    let Some(busiest) = metrics.into_iter().max_by(|a, b| a.1.total_cmp(&b.1)) else {
        return none(Verdict::Unknown);
    };
    let verdict = if busiest.1 >= warn_percent {
        Verdict::Safe
    } else if busiest.1 >= rules::IDLE_PERCENT {
        Verdict::Close
    } else {
        Verdict::AtRisk
    };
    Assessment {
        verdict,
        busiest: Some(busiest),
    }
}
