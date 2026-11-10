//! The report as a table.

use std::fmt::Write as _;

use oci_free_keeper_core::report::{Report, Row};
use oci_free_keeper_core::rules;
use oci_free_keeper_core::verdict::Verdict;

fn pct(v: Option<f64>) -> String {
    v.map_or("—".into(), |v| format!("{v:.1}%"))
}

fn explain(r: &Row, warn: f64) -> String {
    match (r.verdict, r.busiest) {
        (Verdict::Safe, Some((m, _))) => format!("{m} keeps it busy"),
        (Verdict::Close, Some((m, v))) => {
            format!("busiest is {m} at {v:.1}%, warning below {warn}%")
        }
        (Verdict::AtRisk, _) => format!(
            "all metrics under {}%, idle by Oracle's rule",
            rules::IDLE_PERCENT
        ),
        (Verdict::Unknown, _) => r.note.clone().unwrap_or_else(|| "no metric data".into()),
        (Verdict::NotSubject, _) => "not an Always Free shape".into(),
        (Verdict::Stopped, _) => format!(
            "{} — start it, or it is not serving anything",
            r.state.to_lowercase()
        ),
        _ => String::new(),
    }
}

/// The human-readable report.
pub fn text(r: &Report) -> String {
    let mut out = String::new();
    let mut head = format!(
        "oci-free-keeper · home region {} · window {} days",
        r.home_region, r.window_days
    );
    if let Some(m) = &r.payment_model {
        head += &format!(" · {m}");
    }
    let _ = writeln!(out, "{head}\n");

    let rows: Vec<[String; 7]> = r
        .instances
        .iter()
        .map(|row| {
            let u = row.usage.as_ref();
            let mem = if rules::memory_counts(&row.shape) {
                pct(u.and_then(|u| u.memory_mean))
            } else {
                "—".into()
            };
            [
                row.name.clone(),
                row.shape.clone(),
                row.state.clone(),
                pct(u.and_then(|u| u.cpu_p95)),
                mem,
                pct(u.and_then(|u| u.network_percent)),
                format!(
                    "{} {} — {}",
                    row.verdict.symbol(),
                    row.verdict.label(),
                    explain(row, r.warn_percent)
                ),
            ]
        })
        .collect();
    let header = [
        "INSTANCE", "SHAPE", "STATE", "CPU p95", "MEM", "NET", "VERDICT",
    ];
    let mut width = header.map(|h| h.chars().count());
    for row in &rows {
        for (w, cell) in width.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let line = |cells: &[String]| -> String {
        let mut s = String::new();
        for (i, (cell, w)) in cells.iter().zip(width).enumerate() {
            if i == cells.len() - 1 {
                s += cell;
            } else {
                // Numbers right-aligned, text left-aligned.
                let pad = " ".repeat(w - cell.chars().count());
                if (3..=5).contains(&i) {
                    s += &format!("{pad}{cell}   ")
                } else {
                    s += &format!("{cell}{pad}   ")
                }
            }
        }
        s
    };
    let _ = writeln!(out, "{}", line(&header.map(String::from)));
    for row in &rows {
        let _ = writeln!(out, "{}", line(row));
    }
    if r.instances.is_empty() {
        let _ = writeln!(out, "(no instances in the home region)");
    }

    let short: Vec<String> = r
        .instances
        .iter()
        .filter_map(|row| {
            row.usage
                .as_ref()
                .filter(|u| u.days_of_data < 6.5)
                .map(|u| format!("{} has {:.1} days of data", row.name, u.days_of_data))
        })
        .collect();
    if !short.is_empty() {
        let _ = writeln!(
            out,
            "\nNote: {} — the verdict firms up after 7 days.",
            short.join("; ")
        );
    }
    if r.is_payg() && r.instances.iter().any(|row| row.verdict == Verdict::AtRisk) {
        let _ = writeln!(
            out,
            "\nNote: this is a pay-as-you-go account. Oracle's idle rule is written for Always Free \
             accounts, and reclamation has not been observed on PAYG — treat \"At risk\" as a heads-up."
        );
    }
    out
}
