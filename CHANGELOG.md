# Changelog

## 0.4.0 — 2026-11-10

- `watch --auto-start`: starts an Always Free instance again when it stops while watched — Oracle
  stops idle ones. Never one that was stopped before `watch` first looked, never another shape;
  the freeform tag `oci-free-keeper: no-auto-start` opts an instance out. A failed start is
  notified, not retried.
- The `watch` state file is per tenancy and profile now, so two profiles of one tenancy no longer
  share it; the old file is taken over by the DEFAULT profile.

## 0.3.0 — 2026-11-10

- `backup`: a full backup of each Always Free instance's boot volume, tagged
  `managed-by: oci-free-keeper`. Keeps the newest `--keep` (default 1) of its own per instance and
  deletes older ones only after the new one is ready; never touches other backups and never goes
  above the 5 free (boot and block together). `--dry-run`, `--if-older-than 7d` for cron,
  `--instance`. The first command that writes to the account.
- `watch --backup 7d`: the same from the watch loop, with a notification when a backup is ready
  or cannot be made.
- README: the IAM policy `backup` needs, checked on a real tenancy.

## 0.2.0 — 2026-11-10

- README: links to the other DeepLab OCI tools and the crate they share.
- `watch`: looks every hour (`--interval`) and notifies when an instance is gone, stopped or
  running again, or its idle verdict changes — only on changes, remembered in the cache folder.
  A gone instance's leftover boot volume is pointed out with how to bring it back.
  Desktop notifications, and `--webhook` for ntfy, Slack, Discord or Telegram. `--once` for cron:
  exit 2 when an instance is gone, stopped or newly at risk.
- Global options (`--oci-config`, `--profile`) work before the subcommand too.
- README: install with Homebrew, the installer scripts or `cargo install`; how updating works.
- Output into a closed pipe (`… | head -1`) ends quietly with exit code 141 instead of a panic.

## 0.1.2 — 2026-11-10

- Update notice: at most once a day the binary checks GitHub for a newer release and
  prints how to upgrade — the command (brew, cargo or the installer) highlighted — on stderr.
  It never delays a run by more than 1.5 s; off in CI, without a terminal, or with
  `OCI_FREE_KEEPER_NO_UPDATE_CHECK=1`.

## 0.1.1 — 2026-11-10

- `check`: the VERDICT column is short (`✓ Safe (memory)`, `✗ At risk`); explanations for anything
  that needs attention go under the table, so the report fits a 100-column terminal.
- README: a GIF of a real run against a local mock (`assets/demo/`) instead of sample text.
- README: icon and badges (CI, crates.io, license, stars).
- README: a Support section — star the repo, or buy me a coffee.
- README: one command per code block, so each copies on its own.
- README: keeper is about keeping the instance, on any account — the idle rule is one signal;
  `watch` (stopped or gone) and boot-volume backups are next.

## 0.1.0 — 2026-11-10

First version: `check`. Read-only — GET requests and Monitoring queries only.

- Every Always Free instance in the home region, all compartments: CPU (95th percentile of
  5-minute means), memory (A1 only) and network over 7 days, against Oracle's 20% idle rule.
- Verdicts Safe / Close / At risk / Stopped / Unknown / Not subject, and which metric keeps an
  instance safe. Warning line 25% by default (`--warn-threshold`).
- Network throughput from the byte counters, skipping reboots; as a share of the shape's bandwidth.
- A note on pay-as-you-go accounts, where reclamation has not been observed.
- `--json`, `--instance`, exit codes 0 / 1 / 2 / 3.
