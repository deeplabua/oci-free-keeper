//! `backup`: a boot-volume backup per instance, within the 5 free, rotated.
//!
//! keeper only ever deletes backups it made itself — those with the freeform tag
//! [`TAG_KEY`] = [`TAG_VALUE`]. Planning is pure; the requests live in [`crate::api`].

use serde::{Deserialize, Serialize};

/// Freeform tag on every backup keeper makes. Backups without it are never touched.
pub const TAG_KEY: &str = "managed-by";
/// Its value.
pub const TAG_VALUE: &str = "oci-free-keeper";

/// Boot and block volume backups that are free, all together (Always Free, 10 November 2026).
pub const FREE_BACKUPS: usize = 5;

/// A boot volume backup, as `ListBootVolumeBackups` returns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    /// OCID.
    pub id: String,
    /// Name in the console.
    #[serde(default)]
    pub display_name: Option<String>,
    /// The boot volume it was taken of.
    #[serde(default)]
    pub boot_volume_id: Option<String>,
    /// `CREATING`, `AVAILABLE`, `TERMINATING`, `TERMINATED`, `FAULTY`, ...
    pub lifecycle_state: String,
    /// RFC 3339; sorts as text.
    #[serde(default)]
    pub time_created: String,
    /// Freeform tags.
    #[serde(default)]
    pub freeform_tags: std::collections::BTreeMap<String, String>,
}

impl Backup {
    /// Made by keeper.
    pub fn ours(&self) -> bool {
        self.freeform_tags.get(TAG_KEY).map(String::as_str) == Some(TAG_VALUE)
    }

    /// Counts against the free 5: not terminated.
    pub fn live(&self) -> bool {
        !matches!(self.lifecycle_state.as_str(), "TERMINATED" | "TERMINATING")
    }
}

/// An instance whose boot volume is to be backed up.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// Instance name.
    pub instance: String,
    /// Its boot volume.
    pub boot_volume_id: String,
}

/// What to do for one target.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Take a backup, then delete these older ones of ours (oldest first) once it is available.
    Create {
        /// The instance.
        target: Target,
        /// Ours, beyond `keep`, to delete after the new one is available.
        rotate: Vec<Backup>,
    },
    /// Nothing to do, and why.
    Skip {
        /// The instance.
        target: Target,
        /// e.g. "a backup from 2026-11-09 is newer than 7d".
        why: String,
    },
}

/// Plan backups: one new backup per target, keeping `keep` of ours per volume afterwards.
///
/// - `backups`: every backup in the home region (boot and block), ours or not.
/// - `fresh_after`: skip a target whose newest available backup of ours was created after this
///   RFC 3339 time (`--if-older-than`).
/// - Never goes above [`FREE_BACKUPS`]: a target that would need a sixth backup is skipped —
///   keeper does not delete first, so a volume is never left without its last backup.
pub fn plan(
    targets: &[Target],
    backups: &[Backup],
    other_live: usize,
    keep: usize,
    fresh_after: Option<&str>,
) -> Vec<Step> {
    let keep = keep.max(1);
    let mut live = other_live + backups.iter().filter(|b| b.live()).count();
    let mut steps = Vec::new();
    for t in targets {
        let mut ours: Vec<&Backup> = backups
            .iter()
            .filter(|b| b.ours() && b.live())
            .filter(|b| b.boot_volume_id.as_deref() == Some(t.boot_volume_id.as_str()))
            .collect();
        ours.sort_by(|a, b| b.time_created.cmp(&a.time_created));
        if let (Some(after), Some(newest)) = (
            fresh_after,
            ours.iter().find(|b| b.lifecycle_state == "AVAILABLE"),
        ) && newest.time_created.as_str() > after
        {
            steps.push(Step::Skip {
                target: t.clone(),
                why: format!(
                    "its newest backup, from {}, is recent enough",
                    newest
                        .time_created
                        .get(..10)
                        .unwrap_or(&newest.time_created)
                ),
            });
            continue;
        }
        if ours.iter().any(|b| b.lifecycle_state == "CREATING") {
            steps.push(Step::Skip {
                target: t.clone(),
                why: "a backup of it is being created right now".into(),
            });
            continue;
        }
        if live + 1 > FREE_BACKUPS {
            steps.push(Step::Skip {
                target: t.clone(),
                why: format!(
                    "{live} of {FREE_BACKUPS} free backups are in use — a new one would be billed; \
                     delete an old one in the console, or lower --keep"
                ),
            });
            continue;
        }
        // After the new one: keep `keep` in total, so `keep - 1` of the existing ones.
        let rotate: Vec<Backup> = ours.iter().skip(keep - 1).map(|b| (*b).clone()).collect();
        live = live + 1 - rotate.len();
        steps.push(Step::Create {
            target: t.clone(),
            rotate,
        });
    }
    steps
}

/// The name of a new backup: `deeplab-web 2026-11-10 (oci-free-keeper)`.
pub fn backup_name(instance: &str, today: &str) -> String {
    format!("{instance} {today} (oci-free-keeper)")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(name: &str) -> Target {
        Target {
            instance: name.into(),
            boot_volume_id: format!("bv-{name}"),
        }
    }

    fn backup(volume: &str, created: &str, ours: bool, state: &str) -> Backup {
        let mut tags = std::collections::BTreeMap::new();
        if ours {
            tags.insert(TAG_KEY.into(), TAG_VALUE.into());
        }
        Backup {
            id: format!("b-{volume}-{created}"),
            display_name: None,
            boot_volume_id: Some(format!("bv-{volume}")),
            lifecycle_state: state.into(),
            time_created: format!("{created}T03:00:00.000Z"),
            freeform_tags: tags,
        }
    }

    #[test]
    fn first_backup_of_two_instances() {
        let steps = plan(&[target("arm"), target("micro")], &[], 0, 1, None);
        assert_eq!(
            steps,
            vec![
                Step::Create {
                    target: target("arm"),
                    rotate: vec![]
                },
                Step::Create {
                    target: target("micro"),
                    rotate: vec![]
                },
            ]
        );
    }

    #[test]
    fn keeps_n_of_ours_and_never_touches_others() {
        let backups = vec![
            backup("arm", "2026-10-01", true, "AVAILABLE"),
            backup("arm", "2026-11-01", true, "AVAILABLE"),
            backup("arm", "2026-09-01", false, "AVAILABLE"),
        ];
        let steps = plan(&[target("arm")], &backups, 0, 2, None);
        let Step::Create { rotate, .. } = &steps[0] else {
            panic!("{steps:?}")
        };
        // Keep 2: the new one and 2026-11-01; delete 2026-10-01; the untagged one stays.
        assert_eq!(rotate, &vec![backups[0].clone()]);
    }

    #[test]
    fn never_a_sixth_backup() {
        let backups: Vec<Backup> = (1..=5)
            .map(|d| backup("other", &format!("2026-11-0{d}"), false, "AVAILABLE"))
            .collect();
        let steps = plan(&[target("arm")], &backups, 0, 1, None);
        assert!(
            matches!(&steps[0], Step::Skip { why, .. } if why.contains("5 of 5")),
            "{steps:?}"
        );
        // Block volume backups count too.
        let steps = plan(&[target("arm")], &backups[..4], 1, 1, None);
        assert!(matches!(&steps[0], Step::Skip { .. }));
    }

    #[test]
    fn rotation_makes_room_for_the_next_target() {
        // 4 live: 2 of ours (arm, micro) + 2 others. Each new one replaces ours: stays at 4.
        let backups = vec![
            backup("arm", "2026-11-01", true, "AVAILABLE"),
            backup("micro", "2026-11-01", true, "AVAILABLE"),
            backup("x", "2026-01-01", false, "AVAILABLE"),
            backup("y", "2026-01-01", false, "AVAILABLE"),
        ];
        let steps = plan(&[target("arm"), target("micro")], &backups, 0, 1, None);
        assert!(
            steps
                .iter()
                .all(|s| matches!(s, Step::Create { rotate, .. } if rotate.len() == 1)),
            "{steps:?}"
        );
    }

    #[test]
    fn recent_enough_or_in_progress_is_skipped() {
        let backups = vec![
            backup("arm", "2026-11-08", true, "AVAILABLE"),
            backup("micro", "2026-11-10", true, "CREATING"),
        ];
        let steps = plan(
            &[target("arm"), target("micro")],
            &backups,
            0,
            1,
            Some("2026-11-03T00:00:00Z"),
        );
        assert!(matches!(&steps[0], Step::Skip { why, .. } if why.contains("2026-11-08")));
        assert!(matches!(&steps[1], Step::Skip { why, .. } if why.contains("being created")));
        // Older than the line: back up again.
        let steps = plan(
            &[target("arm")],
            &backups[..1],
            0,
            1,
            Some("2026-11-09T00:00:00Z"),
        );
        assert!(matches!(&steps[0], Step::Create { .. }));
    }

    #[test]
    fn names() {
        assert_eq!(
            backup_name("arm-free-1", "2026-11-10"),
            "arm-free-1 2026-11-10 (oci-free-keeper)"
        );
    }
}
