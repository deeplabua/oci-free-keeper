//! The idle rule, branch by branch.

use oci_free_keeper_core::model::Usage;
use oci_free_keeper_core::verdict::{Verdict, assess};

const A1: &str = "VM.Standard.A1.Flex";
const MICRO: &str = "VM.Standard.E2.1.Micro";

fn usage(cpu: f64, mem: f64, net: f64) -> Usage {
    Usage {
        cpu_p95: Some(cpu),
        memory_mean: Some(mem),
        network_bps: Some(0.0),
        network_percent: Some(net),
        days_of_data: 7.0,
    }
}

fn v(shape: &str, u: Usage) -> (Verdict, Option<&'static str>) {
    let a = assess(shape, "RUNNING", Some(&u), 25.0);
    (a.verdict, a.busiest.map(|b| b.0))
}

#[test]
fn one_busy_metric_is_enough() {
    assert_eq!(
        v(A1, usage(2.0, 31.0, 0.1)),
        (Verdict::Safe, Some("memory"))
    );
    assert_eq!(v(A1, usage(40.0, 5.0, 0.1)), (Verdict::Safe, Some("cpu")));
    assert_eq!(
        v(MICRO, usage(1.0, 1.0, 30.0)),
        (Verdict::Safe, Some("network"))
    );
}

#[test]
fn between_20_and_the_warning_line_is_close() {
    assert_eq!(v(A1, usage(22.0, 5.0, 0.0)).0, Verdict::Close);
    assert_eq!(v(A1, usage(20.0, 5.0, 0.0)).0, Verdict::Close);
}

#[test]
fn everything_under_20_is_at_risk() {
    assert_eq!(v(A1, usage(19.9, 19.9, 19.9)).0, Verdict::AtRisk);
}

#[test]
fn memory_does_not_count_on_e2_micro() {
    // 47% memory would save an A1, not a micro.
    assert_eq!(
        v(MICRO, usage(0.8, 47.0, 0.0)),
        (Verdict::AtRisk, Some("cpu"))
    );
    assert_eq!(
        v(A1, usage(0.8, 47.0, 0.0)),
        (Verdict::Safe, Some("memory"))
    );
}

#[test]
fn shape_state_and_data_come_first() {
    assert_eq!(
        assess("VM.Standard.E4.Flex", "RUNNING", None, 25.0).verdict,
        Verdict::NotSubject
    );
    assert_eq!(assess(A1, "STOPPED", None, 25.0).verdict, Verdict::Stopped);
    assert_eq!(assess(A1, "RUNNING", None, 25.0).verdict, Verdict::Unknown);
    let empty = Usage::default();
    assert_eq!(
        assess(A1, "RUNNING", Some(&empty), 25.0).verdict,
        Verdict::Unknown
    );
}

#[test]
fn the_warning_line_is_configurable() {
    let u = usage(30.0, 1.0, 0.0);
    assert_eq!(assess(A1, "RUNNING", Some(&u), 25.0).verdict, Verdict::Safe);
    assert_eq!(
        assess(A1, "RUNNING", Some(&u), 40.0).verdict,
        Verdict::Close
    );
}

#[test]
fn severities_drive_the_exit_code() {
    assert_eq!(Verdict::Safe.severity(), 0);
    assert_eq!(Verdict::NotSubject.severity(), 0);
    assert_eq!(Verdict::Close.severity(), 1);
    assert_eq!(Verdict::Unknown.severity(), 1);
    assert_eq!(Verdict::AtRisk.severity(), 2);
    assert_eq!(Verdict::Stopped.severity(), 2);
}
