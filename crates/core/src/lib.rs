//! `oci-free-keeper-core` — how close Oracle Cloud Always Free instances are to being
//! reclaimed as idle. Reads only; changes nothing.
//!
//! - [`api::collect`]: instances of the home region and 7 days of their metrics
//! - [`verdict::assess`]: Safe / Close / At risk / Unknown / Not subject / Stopped
//! - [`report::check`]: a [`report::Report`] for the CLI
//! - [`rules`]: Oracle's numbers, with the date they were checked
//! - [`watch`]: what changed since the last run — gone, stopped, started, verdict changes

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod api;
pub mod model;
pub mod report;
pub mod rules;
pub mod stats;
pub mod time;
pub mod verdict;
pub mod watch;
