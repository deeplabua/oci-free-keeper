//! `backup`: a full backup of each Always Free instance's boot volume, within the 5 free backups,
//! keeping `--keep` of keeper's own per volume. The only command that writes to the account.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use clap::Args as ClapArgs;
use oci_free_keeper_core::backup::{self, Step, Target};
use oci_free_keeper_core::{api, rules, time};
use oci_request_signer::Client;

/// `backup` options.
#[derive(Debug, Clone, ClapArgs)]
pub struct BackupArgs {
    /// Only instances whose name or OCID contains this.
    #[arg(long, value_name = "NAME|OCID")]
    pub instance: Option<String>,

    /// Backups of keeper's own to keep per instance, the new one included. Older ones are deleted
    /// once the new one is available; backups keeper did not make are never touched.
    #[arg(long, default_value_t = 1, value_name = "N")]
    pub keep: usize,

    /// Skip an instance whose newest backup is younger than this: 7d, 24h... For cron.
    #[arg(long, value_name = "DURATION")]
    pub if_older_than: Option<String>,

    /// Show what would be done, change nothing.
    #[arg(long)]
    pub dry_run: bool,
}

/// What a run did, for `watch`.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Lines worth a notification: a backup taken, or one that could not be.
    pub news: Vec<String>,
    /// 0 done or nothing to do, 1 skipped for the free limit, 2 a backup failed.
    pub code: u8,
}

/// Plan and, unless `--dry-run`, take the backups. Prints as it goes.
pub fn run(client: &Client, args: &BackupArgs) -> Result<Outcome> {
    if args.keep == 0 {
        bail!("--keep must be at least 1");
    }
    let fresh_after = match &args.if_older_than {
        Some(text) => {
            let age = crate::watch::parse_duration(text)?.as_secs();
            Some(time::rfc3339(time::now().saturating_sub(age)))
        }
        None => None,
    };
    let inv = api::inventory(client)?;
    let mut targets = Vec::new();
    for i in inv
        .instances
        .iter()
        .filter(|i| rules::is_free_shape(&i.shape))
    {
        if let Some(f) = &args.instance
            && !(i.name().contains(f.as_str()) || i.id.contains(f.as_str()))
        {
            continue;
        }
        match api::boot_volume_of(client, &inv.home_region, i)? {
            Some(id) => targets.push(Target {
                instance: i.name(),
                boot_volume_id: id,
            }),
            None => println!("  · {}: no boot volume attached — skipped", i.name()),
        }
    }
    if targets.is_empty() {
        match &args.instance {
            Some(f) => bail!(
                "no Always Free instance in {} matches {f:?}",
                inv.home_region
            ),
            None => {
                println!(
                    "No Always Free instances in {} — nothing to back up.",
                    inv.home_region
                );
                return Ok(Outcome::default());
            }
        }
    }

    let (backups, block) = api::backups(client, &inv.home_region, &inv.compartments)?;
    let steps = backup::plan(&targets, &backups, block, args.keep, fresh_after.as_deref());
    let today = &time::rfc3339(time::now())[..10];
    let mut out = Outcome::default();
    for step in steps {
        match step {
            Step::Skip { target, why } => {
                let limit = why.contains("free backups are in use");
                println!(
                    "  {} {}: skipped — {why}",
                    if limit { "⚠" } else { "·" },
                    target.instance
                );
                if limit {
                    out.code = out.code.max(1);
                    out.news
                        .push(format!("{}: no backup — {why}", target.instance));
                }
            }
            Step::Create { target, rotate } => {
                let name = backup::backup_name(&target.instance, today);
                if args.dry_run {
                    println!("  would back up {} as \"{name}\"", target.instance);
                    for b in &rotate {
                        println!("    then delete {}", label(b));
                    }
                    continue;
                }
                match take(client, &inv.home_region, &target, &name) {
                    Ok(()) => {
                        out.news
                            .push(format!("{}: backup \"{name}\" is ready", target.instance));
                        for b in &rotate {
                            match api::delete_backup(client, &inv.home_region, &b.id) {
                                Ok(()) => println!("    deleted the older {}", label(b)),
                                Err(e) => eprintln!("    could not delete {}: {e}", label(b)),
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  ✗ {}: {e:#}", target.instance);
                        out.news
                            .push(format!("{}: backup failed — {e}", target.instance));
                        out.code = 2;
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Create one backup and wait until it is available.
fn take(client: &Client, region: &str, target: &Target, name: &str) -> Result<()> {
    println!("  backing up {} …", target.instance);
    let b = api::create_backup(client, region, &target.boot_volume_id, name)?;
    let started = Instant::now();
    loop {
        let now = api::get_backup(client, region, &b.id)?;
        match now.lifecycle_state.as_str() {
            "AVAILABLE" => {
                println!(
                    "  ✓ {}: \"{name}\" ready in {}s",
                    target.instance,
                    started.elapsed().as_secs()
                );
                return Ok(());
            }
            "CREATING" | "REQUEST_RECEIVED" => {}
            other => bail!("the backup is {other}"),
        }
        if started.elapsed() > Duration::from_secs(2 * 3600) {
            bail!("still not available after 2 hours — check the console");
        }
        std::thread::sleep(Duration::from_secs(10));
    }
}

fn label(b: &backup::Backup) -> String {
    format!(
        "\"{}\" from {}",
        b.display_name.as_deref().unwrap_or(&b.id),
        b.time_created.get(..10).unwrap_or(&b.time_created)
    )
}
