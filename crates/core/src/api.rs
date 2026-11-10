//! Reads instances and their metrics — GET requests and Monitoring queries only.

use oci_request_signer::{Client, Method, OCI_DOMAIN, Result, encode};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::model::{Instance, Usage};
use crate::{rules, stats, time};

/// One instance and its utilisation (or why it could not be read).
#[derive(Debug, Clone)]
pub struct Observed {
    /// The instance.
    pub instance: Instance,
    /// Utilisation over the window; `Err` if Monitoring refused or returned nothing usable.
    pub usage: std::result::Result<Usage, String>,
}

/// What `check` looks at.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Home region — Always Free instances live only there.
    pub home_region: String,
    /// `PAYG`, ... from the Organizations API, if readable.
    pub payment_model: Option<String>,
    /// Every non-terminated instance in the home region, all compartments.
    pub instances: Vec<Observed>,
    /// End of the window, seconds since the epoch.
    pub window_end: u64,
    /// The root compartment and every active one under it.
    pub compartments: Vec<String>,
}

/// A boot volume that outlived its instance.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Leftover {
    /// e.g. `arm-free-1 (Boot Volume)`.
    pub name: String,
    /// OCID.
    pub id: String,
    /// Size, GB.
    pub size_gb: Option<f64>,
}

/// Boot volumes in the home region whose name starts with `instance_name` — by default OCI names
/// them `<instance> (Boot Volume)`, and they can outlive a terminated instance.
pub fn leftover_boot_volumes(
    client: &Client,
    snapshot: &Snapshot,
    instance_name: &str,
) -> Result<Vec<Leftover>> {
    let home = client.in_region(snapshot.home_region.clone());
    let tenancy = encode(home.tenancy());
    let ads: Vec<Value> = list_all(
        &home,
        "identity",
        &format!("/20160918/availabilityDomains?compartmentId={tenancy}"),
    )?;
    let mut found = Vec::new();
    for ad in ads.iter().filter_map(|a| a["name"].as_str()) {
        for c in &snapshot.compartments {
            let path = format!(
                "/20160918/bootVolumes?compartmentId={}&availabilityDomain={}",
                encode(c),
                encode(ad)
            );
            for v in list_all::<Value>(&home, "iaas", &path)? {
                let name = v["displayName"].as_str().unwrap_or_default();
                if v["lifecycleState"] == "AVAILABLE" && name.starts_with(instance_name) {
                    found.push(Leftover {
                        name: name.to_owned(),
                        id: v["id"].as_str().unwrap_or_default().to_owned(),
                        size_gb: v["sizeInGBs"].as_f64(),
                    });
                }
            }
        }
    }
    Ok(found)
}

/// Read the instances of the home region and their last 7 days of metrics, ending `now`.
pub fn collect(client: &Client, now: u64) -> Result<Snapshot> {
    let tenancy = client.tenancy().to_owned();
    let regions: Vec<Value> = client.get(
        "identity",
        &format!("/20160918/tenancies/{tenancy}/regionSubscriptions"),
    )?;
    let home_region = regions
        .iter()
        .find(|r| r["isHomeRegion"] == true)
        .and_then(|r| r["regionName"].as_str())
        .unwrap_or(client.region())
        .to_owned();
    let home = client.in_region(home_region.clone());

    let mut compartments = vec![tenancy.clone()];
    let path = format!(
        "/20160918/compartments?compartmentId={}&compartmentIdInSubtree=true&accessLevel=ACCESSIBLE&lifecycleState=ACTIVE",
        encode(&tenancy)
    );
    for c in list_all::<Value>(&home, "identity", &path)? {
        if let Some(id) = c["id"].as_str() {
            compartments.push(id.to_owned());
        }
    }
    let mut instances = Vec::new();
    for c in &compartments {
        let list: Vec<Instance> = list_all(
            &home,
            "iaas",
            &format!("/20160918/instances?compartmentId={}", encode(c)),
        )?;
        instances.extend(list.into_iter().filter(Instance::exists));
    }

    // Snap the window to whole 5-minute steps so consecutive runs line up.
    let end = now - now % rules::RESOLUTION_SECS;
    let start = end - rules::WINDOW_DAYS * 86_400;
    let observed = instances
        .into_iter()
        .map(|instance| {
            let usage =
                if rules::is_free_shape(&instance.shape) && instance.lifecycle_state == "RUNNING" {
                    usage(&home, &instance, start, end)
                } else {
                    Ok(Usage::default())
                };
            Observed { instance, usage }
        })
        .collect();

    let payment_model = home
        .on_domain(OCI_DOMAIN)
        .get::<Value>(
            "organizations",
            &format!("/20230401/subscriptions?compartmentId={}", encode(&tenancy)),
        )
        .ok()
        .and_then(|v| {
            v["items"]
                .as_array()?
                .iter()
                .find_map(|s| s["paymentModel"].as_str().map(str::to_owned))
        });

    Ok(Snapshot {
        home_region,
        payment_model,
        instances: observed,
        window_end: end,
        compartments,
    })
}

/// CPU p95, memory mean, network mean over the window.
fn usage(
    client: &Client,
    i: &Instance,
    start: u64,
    end: u64,
) -> std::result::Result<Usage, String> {
    let series = |metric: &str, agg: &str| -> std::result::Result<Vec<f64>, String> {
        let body = json!({
            "namespace": "oci_computeagent",
            "query": format!("{metric}[5m]{{resourceId = \"{}\"}}.{agg}", i.id),
            "startTime": time::rfc3339(start),
            "endTime": time::rfc3339(end),
        });
        let path = format!(
            "/20180401/metrics/actions/summarizeMetricsData?compartmentId={}",
            encode(&i.compartment_id)
        );
        let body = serde_json::to_vec(&body).expect("the query always serializes");
        let v: Value = client
            .request(Method::Post, "telemetry", &path, Some(&body), &[])
            .and_then(|r| r.json())
            .map_err(|e| e.to_string())?;
        Ok(v.as_array()
            .and_then(|a| a.first())
            .and_then(|s| s["aggregatedDatapoints"].as_array())
            .map(|d| d.iter().filter_map(|p| p["value"].as_f64()).collect())
            .unwrap_or_default())
    };

    let cpu = series("CpuUtilization", "mean()")?;
    if cpu.is_empty() {
        return Err(
            "no metrics — is the Compute Instance Monitoring plugin of Oracle Cloud Agent enabled?"
                .into(),
        );
    }
    let memory = series("MemoryUtilization", "mean()")?;
    // Byte counters: throughput is the difference between readings.
    let rate = |metric| -> std::result::Result<Option<f64>, String> {
        Ok(stats::mean(&stats::counter_rates_bps(
            &series(metric, "max()")?,
            rules::RESOLUTION_SECS,
        )))
    };
    let network_bps = match (rate("NetworksBytesIn")?, rate("NetworksBytesOut")?) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    let bandwidth = rules::network_bandwidth_bps(&i.shape, i.ocpus());
    Ok(Usage {
        cpu_p95: stats::percentile(&cpu, 0.95),
        memory_mean: stats::mean(&memory),
        network_bps,
        network_percent: network_bps.zip(bandwidth).map(|(bps, bw)| bps / bw * 100.0),
        days_of_data: cpu.len() as f64 * rules::RESOLUTION_SECS as f64 / 86_400.0,
    })
}

/// GET a JSON array, following `opc-next-page`.
fn list_all<T: DeserializeOwned>(client: &Client, service: &str, path: &str) -> Result<Vec<T>> {
    let mut all = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let sep = if path.contains('?') { '&' } else { '?' };
        let url = match &page {
            Some(p) => format!("{path}{sep}page={}", encode(p)),
            None => path.to_owned(),
        };
        let resp = client.request(Method::Get, service, &url, None, &[])?;
        all.extend(resp.json::<Vec<T>>()?);
        match resp.header("opc-next-page") {
            Some(next) if !next.is_empty() => page = Some(next.to_owned()),
            _ => return Ok(all),
        }
    }
}
