<p align="center"><img src=".github/oci-free-keeper-256.png" width="128" height="128" alt="oci-free-keeper icon"></p>

# oci-free-keeper

[![CI](https://img.shields.io/github/actions/workflow/status/deeplabua/oci-free-keeper/ci.yml?branch=main&logo=github&label=CI)](https://github.com/deeplabua/oci-free-keeper/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/oci-free-keeper?label=crates.io&color=e37933)](https://crates.io/crates/oci-free-keeper)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Stars](https://img.shields.io/github/stars/deeplabua/oci-free-keeper?style=flat&logo=github&label=Star)](https://github.com/deeplabua/oci-free-keeper/stargazers)

**Keep your Oracle Cloud instance.** After a long hunt for a free ARM instance, know how close it
is to Oracle's idle rule — and, coming next, the moment it is stopped or gone, with a backup of its
boot volume to bring it back.

One binary, no OCI CLI, no Python. Today `check` reads 7 days of metrics for every instance in your
home region and compares them with the idle rule. **Read-only: it changes nothing in your account.**

Part of [DeepLab](https://deeplab.tools) — small, sharp tools that run locally. A sibling of
[oci-arm-catcher](https://github.com/deeplabua/oci-arm-catcher) (catch a free ARM instance) and
[oci-free-audit](https://github.com/deeplabua/oci-free-audit) (make sure it stays free).

![oci-free-keeper: one instance kept busy by memory, one idle by Oracle's rule](assets/demo.gif)

<sub>Recorded against a local mock of the OCI API (`assets/demo/`); the instances are made up.</sub>

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

The report (same as `oci-free-keeper check`):

```sh
oci-free-keeper
```

As JSON, for scripts and cron:

```sh
oci-free-keeper --json
```

A wider safety margin than the default 25%:

```sh
oci-free-keeper --warn-threshold 30
```

Only instances whose name or OCID contains `arm`:

```sh
oci-free-keeper --instance arm
```

Another profile in `~/.oci/config`:

```sh
oci-free-keeper --profile OTHER
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

`watch` — a notification the moment an instance is stopped or gone (any account, PAYG included),
then on idle-verdict changes; boot-volume backups within the 5 free; `--auto-start` for a stopped
instance. See [`CHANGELOG.md`](CHANGELOG.md).

## Support

If `oci-free-keeper` helped you keep your instance, two things help a lot:

- ⭐ **[Star the repo](https://github.com/deeplabua/oci-free-keeper)** — it's how other people holding on to a free instance find it.
- ☕ **[Buy me a coffee](https://send.monobank.ua/jar/4Hq7auheaa)** — if it saved you some time and you feel like it.

Both are completely optional. The tool is free and always will be.

## License

[MIT](LICENSE)
