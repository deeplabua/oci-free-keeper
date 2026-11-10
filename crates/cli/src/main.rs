//! `oci-free-keeper` — keep Oracle Cloud Always Free instances from being reclaimed as idle.
//!
//! Exit codes (`check`): 0 all safe, 1 close or unknown, 2 at risk or stopped, 3 could not run.

/// `println!` / `print!` that exit quietly when stdout is a closed pipe (`… | head -1`),
/// the way `cat` and `grep` do, instead of panicking. Declared before the modules so it
/// shadows the std macros everywhere in the binary.
macro_rules! println {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if let Err(e) = writeln!(std::io::stdout(), $($arg)*) {
            crate::stdout_failed(e);
        }
    }};
}
macro_rules! print {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        if let Err(e) = write!(std::io::stdout(), $($arg)*) {
            crate::stdout_failed(e);
        }
    }};
}

/// A failed write to stdout: a closed pipe ends the program quietly with 141 (128 + SIGPIPE,
/// what a shell reports for a tool killed by SIGPIPE); anything else is a real error.
fn stdout_failed(e: std::io::Error) -> ! {
    if e.kind() == std::io::ErrorKind::BrokenPipe {
        std::process::exit(141);
    }
    panic!("failed printing to stdout: {e}");
}

mod backup;
mod output;
mod update;
mod watch;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context as _, Result, bail};
use clap::{Args as ClapArgs, Parser, Subcommand};
use oci_free_keeper_core::{api, report, rules, time};
use oci_request_signer::{Client, Profile};

/// Exit code when the check could not run: bad arguments, config, key or network.
const CANNOT_RUN: u8 = 3;

/// Keep your Oracle Cloud Always Free instances from being reclaimed as idle.
///
/// Reads 7 days of metrics for every instance in your home region and compares them with
/// Oracle's rule: CPU (95th percentile), network and memory (A1 only) all below 20% means idle.
/// Read-only. Exit codes: 0 all safe, 1 close or unknown, 2 at risk or stopped, 3 could not run.
#[derive(Debug, Parser)]
#[command(name = "oci-free-keeper", version)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    check: CheckArgs,

    /// OCI config file [default: ~/.oci/config].
    #[arg(long, global = true, env = "OCI_CONFIG_FILE", value_name = "FILE")]
    oci_config: Option<PathBuf>,

    /// Profile in the OCI config file.
    #[arg(
        long,
        global = true,
        env = "OCI_CLI_PROFILE",
        default_value = "DEFAULT"
    )]
    profile: String,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// How close each Always Free instance is to the idle rule (the default).
    Check(CheckArgs),
    /// Look every hour and notify when an instance stops, disappears, or its verdict changes.
    Watch(watch::WatchArgs),
    /// Back up each Always Free instance's boot volume, within the 5 free backups. Writes to the
    /// account: creates backups, and deletes older ones keeper made itself.
    Backup(backup::BackupArgs),
}

#[derive(Debug, Clone, ClapArgs)]
struct CheckArgs {
    /// Print the report as JSON.
    #[arg(long)]
    json: bool,

    /// Warn when the busiest metric is below this, percent (the rule's line is 20).
    #[arg(long, env = "OCI_FREE_KEEPER_WARN_THRESHOLD", default_value_t = rules::DEFAULT_WARN_PERCENT, value_name = "PERCENT")]
    warn_threshold: f64,

    /// Only instances whose name or OCID contains this.
    #[arg(long, value_name = "NAME|OCID")]
    instance: Option<String>,
}

fn main() -> ExitCode {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(e) => {
            let _ = e.print();
            // Not clap's default 2, which here means "at risk".
            return if e.use_stderr() {
                ExitCode::from(CANNOT_RUN)
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    let mut update = update::UpdateCheck::start();
    let code = match run(&args) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(CANNOT_RUN)
        }
    };
    update.report(std::time::Duration::from_millis(1500));
    code
}

fn run(args: &Args) -> Result<u8> {
    let check = match &args.command {
        Some(Command::Check(c)) => c,
        Some(Command::Watch(w)) => return watch::run(&client(args)?, &args.profile, w),
        Some(Command::Backup(b)) => return Ok(backup::run(&client(args)?, b)?.code),
        None => &args.check,
    };
    if !(rules::IDLE_PERCENT..=100.0).contains(&check.warn_threshold) {
        bail!(
            "--warn-threshold must be between {} and 100",
            rules::IDLE_PERCENT
        );
    }
    let client = client(args)?;

    let mut snapshot = api::collect(&client, time::now())?;
    if let Some(filter) = &check.instance {
        snapshot.instances.retain(|o| {
            o.instance.name().contains(filter.as_str()) || o.instance.id.contains(filter.as_str())
        });
        if snapshot.instances.is_empty() {
            bail!("no instance in {} matches {filter:?}", snapshot.home_region);
        }
    }
    let report = report::check(&snapshot, check.warn_threshold);
    if check.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", output::text(&report));
    }
    Ok(report.exit_code())
}

/// The signed OCI client for the chosen profile.
fn client(args: &Args) -> Result<Client> {
    let path = match &args.oci_config {
        Some(p) => p.clone(),
        None => {
            Profile::default_path().context("cannot find your home directory; pass --oci-config")?
        }
    };
    let profile = Profile::load(&path, &args.profile)?;
    Ok(Client::builder(&profile)
        .user_agent(concat!("oci-free-keeper/", env!("CARGO_PKG_VERSION")))
        // A local mock of the OCI API for tests and demos.
        .endpoint(std::env::var("OCI_FREE_KEEPER_ENDPOINT").ok())
        .build()?)
}
