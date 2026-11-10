# Changelog

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
