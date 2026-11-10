//! `watch`: look every hour, tell the user when something changes — an instance stopped or gone
//! first of all, then idle-verdict changes. Remembers the last look in the user's cache folder,
//! so `watch --once` from cron behaves the same as the long-running loop.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use clap::Args as ClapArgs;
use oci_free_keeper_core::watch::{Event, State, diff, state_of};
use oci_free_keeper_core::{api, report, rules, time};
use oci_request_signer::Client;

/// `watch` options.
#[derive(Debug, Clone, ClapArgs)]
pub struct WatchArgs {
    /// How often to look: 30m, 1h, 6h... (at least 1m).
    #[arg(
        long,
        env = "OCI_FREE_KEEPER_INTERVAL",
        default_value = "1h",
        value_name = "DURATION"
    )]
    interval: String,

    /// Look once, report what changed since the last look, and exit — for cron or a systemd timer.
    #[arg(long)]
    once: bool,

    /// Also POST each notification to this URL: ntfy, Slack, Discord, Telegram sendMessage, or any
    /// endpoint that takes JSON {"text": "..."}.
    #[arg(long, env = "OCI_FREE_KEEPER_WEBHOOK", value_name = "URL")]
    webhook: Option<String>,

    /// No desktop notifications (they are on by default).
    #[arg(long)]
    no_desktop: bool,

    /// Warn when the busiest metric is below this, percent (the rule's line is 20).
    #[arg(long, env = "OCI_FREE_KEEPER_WARN_THRESHOLD", default_value_t = rules::DEFAULT_WARN_PERCENT, value_name = "PERCENT")]
    warn_threshold: f64,
}

/// Run the loop (or one look with `--once`). Returns the exit code.
pub fn run(client: &Client, args: &WatchArgs) -> Result<u8> {
    let every = parse_duration(&args.interval)?;
    let state_path =
        state_file(client.tenancy()).context("cannot find a cache folder for the watch state")?;
    let notify = Notifier {
        desktop: !args.no_desktop,
        webhook: args.webhook.clone(),
    };
    if !args.once {
        println!(
            "Watching every {} — Ctrl-C to stop. State: {}",
            args.interval,
            state_path.display()
        );
    }
    loop {
        let code = match look(client, args, &state_path, &notify) {
            Ok(code) => code,
            Err(e) => {
                // A network blip or an expired key must not end a long-running watch.
                eprintln!("[{}] could not look: {e:#}", stamp());
                if args.once {
                    return Err(e);
                }
                0
            }
        };
        if args.once {
            return Ok(code);
        }
        std::thread::sleep(every);
    }
}

/// One look: read, compare with the last look, report, remember.
fn look(client: &Client, args: &WatchArgs, state_path: &PathBuf, notify: &Notifier) -> Result<u8> {
    let snapshot = api::collect(client, time::now())?;
    let report = report::check(&snapshot, args.warn_threshold);
    let now = state_of(&report);
    let before: Option<State> = std::fs::read_to_string(state_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let first = before.is_none();
    let events = diff(before.as_ref(), &now);

    let summary: Vec<String> = report
        .instances
        .iter()
        .map(|r| format!("{} {} {}", r.verdict.symbol(), r.name, r.verdict.label()))
        .collect();
    let summary = if summary.is_empty() {
        "no instances".to_owned()
    } else {
        summary.join(" · ")
    };
    println!(
        "[{}] {summary}{}",
        stamp(),
        if first {
            "  (first look — changes are reported from now on)"
        } else {
            ""
        }
    );

    for event in &events {
        let mut text = event.message();
        if let Event::Gone { name, shape } = event {
            text.push_str(&leftover_hint(client, &snapshot, name, shape));
        }
        println!("  {} {text}", if event.urgent() { "✗" } else { "·" });
        if event.notify() {
            notify.send(&text, event.urgent());
        }
    }

    if let Some(dir) = state_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(state_path, serde_json::to_string_pretty(&now)?)
        .with_context(|| format!("cannot save the watch state to {}", state_path.display()))?;
    Ok(if events.iter().any(Event::urgent) {
        2
    } else {
        report.exit_code().min(1)
    })
}

/// Where a gone instance's boot volume still is, and how to bring it back.
fn leftover_hint(client: &Client, snapshot: &api::Snapshot, name: &str, shape: &str) -> String {
    match api::leftover_boot_volumes(client, snapshot, name) {
        Ok(found) if !found.is_empty() => {
            let v = &found[0];
            let tail: String =
                v.id.chars()
                    .rev()
                    .take(6)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
            let catcher = if shape == rules::A1_FLEX {
                " Short on A1 capacity? oci-arm-catcher retries for you."
            } else {
                ""
            };
            format!(
                ". Its boot volume \"{}\" (…{tail}) is still there: Console → Block Storage → Boot Volumes → \
                 Create Instance.{catcher}",
                v.name
            )
        }
        Ok(_) => {
            ". No boot volume of it is left — restore from a backup if you have one.".to_owned()
        }
        Err(_) => String::new(),
    }
}

/// `2026-11-10 18:05Z`.
fn stamp() -> String {
    let t = time::rfc3339(time::now());
    format!("{} {}Z", &t[..10], &t[11..16])
}

/// One state file per tenancy, in the user's cache folder.
fn state_file(tenancy: &str) -> Option<PathBuf> {
    let tail: String = tenancy
        .chars()
        .rev()
        .take(12)
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    dirs::cache_dir().map(|d| d.join("oci-free-keeper").join(format!("watch-{tail}.json")))
}

/// `90s`, `30m`, `1h`, `1d` — at least one minute.
fn parse_duration(text: &str) -> Result<Duration> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (n, unit) = text.split_at(split);
    let n: u64 = n
        .parse()
        .with_context(|| format!("--interval: expected e.g. 30m or 1h, got {text:?}"))?;
    let secs = match unit {
        "s" => n,
        "m" | "" => n * 60,
        "h" => n * 3600,
        "d" => n * 86_400,
        _ => bail!("--interval: unknown unit {unit:?} (use s, m, h or d)"),
    };
    if secs < 60 {
        bail!("--interval must be at least 1m");
    }
    Ok(Duration::from_secs(secs))
}

/// Desktop and webhook notifications. Failures are reported, never fatal.
struct Notifier {
    desktop: bool,
    webhook: Option<String>,
}

impl Notifier {
    fn send(&self, text: &str, urgent: bool) {
        if self.desktop {
            let _ = notify_rust::Notification::new()
                .summary(if urgent {
                    "oci-free-keeper — act now"
                } else {
                    "oci-free-keeper"
                })
                .body(text)
                .show();
        }
        if let Some(url) = &self.webhook {
            let (content_type, body) = webhook_body(url, text);
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(10)))
                .user_agent(concat!("oci-free-keeper/", env!("CARGO_PKG_VERSION")))
                .build()
                .into();
            if let Err(e) = agent
                .post(url)
                .header("content-type", content_type)
                .send(body.as_bytes())
            {
                eprintln!("  webhook failed: {e}");
            }
        }
    }
}

/// What each kind of webhook wants: ntfy takes plain text, Discord `content`, the rest `text`.
fn webhook_body(url: &str, text: &str) -> (&'static str, String) {
    if url.contains("ntfy") {
        ("text/plain", text.to_owned())
    } else if url.contains("discord.com") || url.contains("discordapp.com") {
        (
            "application/json",
            serde_json::json!({ "content": text }).to_string(),
        )
    } else {
        (
            "application/json",
            serde_json::json!({ "text": text }).to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert_eq!(parse_duration("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_duration("90").unwrap(), Duration::from_secs(5400));
        assert_eq!(parse_duration("1d").unwrap(), Duration::from_secs(86_400));
        assert!(parse_duration("30s").is_err());
        assert!(parse_duration("1w").is_err());
        assert!(parse_duration("soon").is_err());
    }

    #[test]
    fn webhook_payloads() {
        assert_eq!(
            webhook_body("https://ntfy.sh/my-topic", "hi"),
            ("text/plain", "hi".into())
        );
        assert_eq!(
            webhook_body("https://discord.com/api/webhooks/1/x", "hi").1,
            r#"{"content":"hi"}"#
        );
        assert_eq!(
            webhook_body("https://hooks.slack.com/services/x", "hi").1,
            r#"{"text":"hi"}"#
        );
        assert_eq!(
            webhook_body(
                "https://api.telegram.org/bot1:x/sendMessage?chat_id=5",
                "hi"
            )
            .1,
            r#"{"text":"hi"}"#
        );
    }

    #[test]
    fn one_state_file_per_tenancy() {
        let a = state_file("ocid1.tenancy.oc1..aaaaaaaabbbbcccc").unwrap();
        let b = state_file("ocid1.tenancy.oc1..aaaaaaaabbbbdddd").unwrap();
        assert_ne!(a, b);
        assert!(a.to_string_lossy().contains("oci-free-keeper"));
    }
}
