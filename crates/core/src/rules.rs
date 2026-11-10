//! Oracle's idle-reclamation rule and the numbers around it — the only place they live.
//!
//! Source: <https://docs.oracle.com/iaas/Content/FreeTier/resourceref.htm>, checked
//! [`CHECKED`]. An Always Free instance may be reclaimed if, over 7 days, CPU (95th percentile),
//! network and memory (A1 only) are all below 20%.

/// When the rule below was last checked against Oracle's page.
pub const CHECKED: &str = "2026-11-10";

/// Below this, a metric counts as idle, percent.
pub const IDLE_PERCENT: f64 = 20.0;

/// Default warning threshold, percent: a margin above [`IDLE_PERCENT`].
pub const DEFAULT_WARN_PERCENT: f64 = 25.0;

/// The window Oracle looks at, days.
pub const WINDOW_DAYS: u64 = 7;

/// Metric resolution: 5-minute points, 2016 in 7 days.
pub const RESOLUTION_SECS: u64 = 300;

/// The Ampere A1 Flex shape.
pub const A1_FLEX: &str = "VM.Standard.A1.Flex";

/// The AMD micro shape.
pub const E2_MICRO: &str = "VM.Standard.E2.1.Micro";

/// Shapes the rule applies to.
pub fn is_free_shape(shape: &str) -> bool {
    shape == A1_FLEX || shape == E2_MICRO
}

/// Memory counts only for A1.
pub fn memory_counts(shape: &str) -> bool {
    shape == A1_FLEX
}

/// What "100% network" means for a shape, bits per second. Oracle does not say; the working
/// hypothesis (PRD, question 1) is the shape's bandwidth: 1 Gbit/s per OCPU on A1, 50 Mbit/s
/// to the internet on E2.1.Micro.
pub fn network_bandwidth_bps(shape: &str, ocpus: f64) -> Option<f64> {
    match shape {
        A1_FLEX => Some(1e9 * ocpus.max(1.0)),
        E2_MICRO => Some(50e6),
        _ => None,
    }
}
