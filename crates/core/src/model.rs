//! What keeper reads from the tenancy.

use serde::{Deserialize, Serialize};

/// A compute instance, as `ListInstances` returns it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    /// OCID.
    pub id: String,
    /// Compartment OCID — the Monitoring query runs there.
    pub compartment_id: String,
    /// Console name; `null` when the caller may only `inspect` instances.
    #[serde(default)]
    pub display_name: Option<String>,
    /// e.g. `VM.Standard.A1.Flex`.
    pub shape: String,
    /// `RUNNING`, `STOPPED`, ...
    pub lifecycle_state: String,
    /// e.g. `Abcd:EU-AMSTERDAM-1-AD-1` — where its boot volume attachment is listed.
    #[serde(default)]
    pub availability_domain: Option<String>,
    /// Freeform tags, e.g. `oci-free-keeper: no-auto-start`.
    #[serde(default)]
    pub freeform_tags: std::collections::BTreeMap<String, String>,
    /// OCPUs and memory.
    #[serde(default)]
    pub shape_config: Option<ShapeConfig>,
}

impl Instance {
    /// The console name, or `instance …lprq` when OCI hides it.
    pub fn name(&self) -> String {
        match self.display_name.as_deref() {
            Some(n) if !n.is_empty() => n.to_owned(),
            _ => {
                let tail: String = self
                    .id
                    .chars()
                    .rev()
                    .take(4)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                format!("instance …{tail}")
            }
        }
    }

    /// OCPUs (0 if unknown).
    pub fn ocpus(&self) -> f64 {
        self.shape_config
            .as_ref()
            .and_then(|c| c.ocpus)
            .unwrap_or(0.0)
    }

    /// Tagged `oci-free-keeper: no-auto-start`.
    pub fn no_auto_start(&self) -> bool {
        let (k, v) = crate::watch::NO_AUTO_START;
        self.freeform_tags.get(k).map(String::as_str) == Some(v)
    }

    /// Not terminated.
    pub fn exists(&self) -> bool {
        !matches!(self.lifecycle_state.as_str(), "TERMINATED" | "TERMINATING")
    }
}

/// OCPUs and memory.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeConfig {
    /// OCPUs.
    pub ocpus: Option<f64>,
    /// Memory, GB. OCI spells it `memoryInGBs`.
    #[serde(rename = "memoryInGBs")]
    pub memory_in_gbs: Option<f64>,
}

/// Utilisation over the window.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Usage {
    /// CPU, 95th percentile of 5-minute means, percent.
    pub cpu_p95: Option<f64>,
    /// Memory, mean, percent.
    pub memory_mean: Option<f64>,
    /// Network, mean of the busier direction, bits per second.
    pub network_bps: Option<f64>,
    /// `network_bps` as a share of the shape's bandwidth, percent.
    pub network_percent: Option<f64>,
    /// Days of CPU data in the window.
    pub days_of_data: f64,
}
