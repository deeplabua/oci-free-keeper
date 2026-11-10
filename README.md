# oci-free-keeper

**Keep your Oracle Cloud Always Free instances from being reclaimed as idle.** Know how close each
instance is to Oracle's idle rule — before Oracle does.

One binary, no OCI CLI, no Python. It reads 7 days of metrics for every instance in your home
region and compares them with the rule. **Read-only: it changes nothing in your account.**

Part of [DeepLab](https://deeplab.tools) — small, sharp tools that run locally. A sibling of
[oci-arm-catcher](https://github.com/deeplabua/oci-arm-catcher) (catch a free ARM instance) and
[oci-free-audit](https://github.com/deeplabua/oci-free-audit) (make sure it stays free).

```
oci-free-keeper · home region eu-amsterdam-1 · window 7 days

INSTANCE     SHAPE                    STATE     CPU p95    MEM    NET   VERDICT
arm-free-1   VM.Standard.A1.Flex      RUNNING      4.1%   31.2%   0.3%   ✓ Safe — memory keeps it busy
micro-1      VM.Standard.E2.1.Micro   RUNNING     12.0%      —   2.1%   ✗ At risk — all metrics under 20%, idle by Oracle's rule
```

## The rule

Oracle [may reclaim](https://docs.oracle.com/iaas/Content/FreeTier/resourceref.htm) an Always Free
instance if, over 7 days, **all** of these are below 20%:

- CPU, 95th percentile;
- network;
- memory — A1 only.

**One metric above 20% is enough.** An A1 with 12 GB that keeps 2.5 GB in use is safe even with
the CPU at zero. Burning CPU to look busy (NeverIdle, LookBusy) is not needed if something useful
already holds memory or traffic — keeper tells you which metric is protecting each instance.

| Verdict | Means |
|---|---|
| ✓ Safe | at least one metric above the warning line (25% by default) |
| ⚠ Close | every metric under 25%, at least one at 20% or more |
| ✗ At risk | every metric under 20%: idle by Oracle's rule |
| ✗ Stopped | not running |
| ? Unknown | no metrics — the Oracle Cloud Agent "Compute Instance Monitoring" plugin is off, or the instance is new |
| — Not subject | not an Always Free shape; the rule does not apply |

How the numbers are computed: CPU is the 95th percentile of 5-minute means over 7 days; memory and
network are 7-day means (lower than a 95th percentile, so keeper warns earlier rather than later).
Oracle does not say what "20% network" is measured against; keeper uses the shape's bandwidth —
1 Gbit/s per OCPU on A1, 50 Mbit/s on E2.1.Micro — and shows the raw bit/s in `--json`.

**Pay-as-you-go accounts.** The rule is written for Always Free accounts. On a PAYG tenancy we
watched, an "idle" A1 has not been reclaimed in five months. keeper still shows the verdict and
adds a note on PAYG accounts.

## Install

Not released yet. From source:

```sh
cargo install --git https://github.com/deeplabua/oci-free-keeper oci-free-keeper
```

It uses the API key in `~/.oci/config` — the same one the OCI CLI and oci-arm-catcher use.

## Use

```sh
oci-free-keeper                       # the report (same as `oci-free-keeper check`)
oci-free-keeper --json                # for scripts and cron
oci-free-keeper --warn-threshold 30   # a wider safety margin
oci-free-keeper --instance arm        # only instances whose name or OCID contains "arm"
oci-free-keeper --profile OTHER       # another profile in ~/.oci/config
```

Exit codes: `0` all safe, `1` close or unknown, `2` at risk or stopped, `3` could not run. Run it
daily from cron and alert on `2`.

## Permissions

Read-only. Checked on a real tenancy on 10 November 2026:

```
Allow group <your-group> to inspect compartments in tenancy
Allow group <your-group> to read instances in tenancy
Allow group <your-group> to read metrics in tenancy
```

`inspect instances` also works, but OCI then hides instance names; keeper shows the end of the
OCID instead. The PAYG note needs the Organizations API (`inspect all-resources` covers it).

## Not yet

`watch` (hourly, with notifications on changes), boot-volume backups within the free limit, and
`--auto-start` for a stopped instance. See [`CHANGELOG.md`](CHANGELOG.md).

## License

[MIT](LICENSE)
