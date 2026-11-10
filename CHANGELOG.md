# Changelog

## Unreleased — 0.1.0

First version: `check`. Read-only — GET requests and Monitoring queries only.

- Every Always Free instance in the home region, all compartments: CPU (95th percentile of
  5-minute means), memory (A1 only) and network over 7 days, against Oracle's 20% idle rule.
- Verdicts Safe / Close / At risk / Stopped / Unknown / Not subject, and which metric keeps an
  instance safe. Warning line 25% by default (`--warn-threshold`).
- Network throughput from the byte counters, skipping reboots; as a share of the shape's bandwidth.
- A note on pay-as-you-go accounts, where reclamation has not been observed.
- `--json`, `--instance`, exit codes 0 / 1 / 2 / 3.
