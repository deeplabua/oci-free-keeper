//! `api::collect` + `report::check` against a local mock of the OCI API.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use oci_free_keeper_core::verdict::Verdict;
use oci_free_keeper_core::{api, report};
use oci_request_signer::{Client, Profile};
use serde_json::{Value, json};

/// 2016 five-minute points of a metric for an instance.
fn series(metric: &str, instance: &str) -> Vec<f64> {
    let n = 2016;
    match (metric, instance) {
        // "busy": CPU p95 ~ 35% → Safe. "idle": everything tiny → At risk.
        ("CpuUtilization", "busy") => (0..n)
            .map(|i| if i % 10 == 0 { 5.0 } else { 35.0 })
            .collect(),
        ("CpuUtilization", "idle") => vec![1.0; n],
        ("MemoryUtilization", _) => vec![8.0; n],
        // Byte counters: +3000 bytes per 5 minutes (80 bit/s), with a reboot reset halfway.
        ("NetworksBytesIn" | "NetworksBytesOut", _) => (0..n)
            .map(|i| {
                if i < n / 2 {
                    3000.0 * i as f64
                } else {
                    3000.0 * (i - n / 2) as f64
                }
            })
            .collect(),
        // "nodata": the agent plugin is off.
        _ => vec![],
    }
}

fn route(
    method: &str,
    path: &str,
    body: &[u8],
) -> (u16, Vec<(&'static str, &'static str)>, String) {
    let ok = |b: String| (200, vec![], b);
    let p = path.split('?').next().unwrap();
    match (method, p) {
        ("GET", "/20160918/tenancies/ocid1.tenancy.oc1..t/regionSubscriptions") => {
            ok(r#"[{"regionName":"eu-amsterdam-1","isHomeRegion":true}]"#.into())
        }
        ("GET", "/20160918/compartments") => ok(r#"[{"id":"ocid1.compartment.oc1..c1"}]"#.into()),
        ("GET", "/20160918/instances") if path.contains("ocid1.compartment.oc1..c1") => ok(
            r#"[{"id":"ocid1.instance.oc1..nodata","compartmentId":"ocid1.compartment.oc1..c1","displayName":"nodata",
                 "shape":"VM.Standard.A1.Flex","lifecycleState":"RUNNING","shapeConfig":{"ocpus":1,"memoryInGBs":6}}]"#.into(),
        ),
        ("GET", "/20160918/instances") if path.contains("page=2") => ok(
            r#"[{"id":"ocid1.instance.oc1..idle","compartmentId":"ocid1.tenancy.oc1..t","displayName":null,
                 "shape":"VM.Standard.E2.1.Micro","lifecycleState":"RUNNING","shapeConfig":{"ocpus":1,"memoryInGBs":1}},
                {"id":"ocid1.instance.oc1..paid","compartmentId":"ocid1.tenancy.oc1..t","displayName":"paid",
                 "shape":"VM.Standard.E4.Flex","lifecycleState":"RUNNING"},
                {"id":"ocid1.instance.oc1..gone","compartmentId":"ocid1.tenancy.oc1..t","displayName":"gone",
                 "shape":"VM.Standard.A1.Flex","lifecycleState":"TERMINATED"}]"#.into(),
        ),
        ("GET", "/20160918/instances") => (
            200,
            vec![("opc-next-page", "2")],
            r#"[{"id":"ocid1.instance.oc1..busy","compartmentId":"ocid1.tenancy.oc1..t","displayName":"busy",
                 "shape":"VM.Standard.A1.Flex","lifecycleState":"RUNNING","shapeConfig":{"ocpus":2,"memoryInGBs":12}}]"#.into(),
        ),
        ("POST", "/20180401/metrics/actions/summarizeMetricsData") => {
            let q: Value = serde_json::from_slice(body).unwrap();
            let query = q["query"].as_str().unwrap();
            let metric = query.split('[').next().unwrap();
            let instance = query.split("ocid1.instance.oc1..").nth(1).unwrap().split('"').next().unwrap();
            let points: Vec<Value> = series(metric, instance)
                .into_iter()
                .map(|v| json!({"timestamp": "2026-11-10T00:00:00Z", "value": v}))
                .collect();
            ok(json!([{"name": metric, "aggregatedDatapoints": points}]).to_string())
        }
        ("GET", "/20230401/subscriptions") => ok(r#"{"items":[{"paymentModel":"PAYG"}]}"#.into()),
        _ => (404, vec![], format!(r#"{{"code":"NotFound","message":"no mock for {method} {path}"}}"#)),
    }
}

fn mock() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                continue;
            }
            let mut parts = line.split_whitespace();
            let (method, path) = (
                parts.next().unwrap().to_owned(),
                parts.next().unwrap().to_owned(),
            );
            let mut len = 0;
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                let h = h.trim_end().to_ascii_lowercase();
                if h.is_empty() {
                    break;
                }
                if let Some(v) = h.strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            log.lock().unwrap().push(format!("{method} {path}"));
            let (status, headers, resp_body) = route(&method, &path, &body);
            let mut resp = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
                resp_body.len()
            );
            for (k, v) in headers {
                resp += &format!("{k}: {v}\r\n");
            }
            resp += "\r\n";
            resp += &resp_body;
            let mut out = stream;
            let _ = out.write_all(resp.as_bytes());
        }
    });
    (base, seen)
}

#[test]
fn checks_every_instance_of_the_home_region() {
    let (base, seen) = mock();
    let profile = Profile {
        user: "ocid1.user.oc1..u".into(),
        fingerprint: "aa:bb".into(),
        tenancy: "ocid1.tenancy.oc1..t".into(),
        region: "eu-amsterdam-1".into(),
        key_file: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test-key.pem"),
        pass_phrase: None,
    };
    let client = Client::builder(&profile).endpoint(base).build().unwrap();
    let snap = api::collect(&client, 1_794_326_700).unwrap();
    let r = report::check(&snap, 25.0);

    let by_name = |n: &str| {
        r.instances
            .iter()
            .find(|i| i.name == n)
            .unwrap_or_else(|| panic!("{n} missing: {r:#?}"))
    };
    assert_eq!(by_name("busy").verdict, Verdict::Safe);
    assert_eq!(by_name("busy").busiest.unwrap().0, "cpu");
    assert_eq!(by_name("instance …idle").verdict, Verdict::AtRisk);
    assert_eq!(by_name("paid").verdict, Verdict::NotSubject);
    let nodata = by_name("nodata");
    assert_eq!(nodata.verdict, Verdict::Unknown);
    assert!(
        nodata
            .note
            .as_deref()
            .unwrap()
            .contains("Compute Instance Monitoring")
    );
    assert!(
        r.instances.iter().all(|i| i.name != "gone"),
        "terminated instances are skipped"
    );

    // Counter reset skipped: 80 bit/s, not a huge negative or positive spike.
    let net = by_name("busy").usage.as_ref().unwrap().network_bps.unwrap();
    assert!((net - 80.0).abs() < 1e-9, "{net}");
    assert_eq!(by_name("busy").usage.as_ref().unwrap().days_of_data, 7.0);

    assert!(r.is_payg());
    assert_eq!(r.exit_code(), 2);

    let seen = seen.lock().unwrap();
    // The paid shape's metrics are not even read.
    assert!(!seen.iter().any(|s| s.contains("paid")));
    // Only reads: GETs and Monitoring queries.
    assert!(
        seen.iter().all(|s| s.starts_with("GET ")
            || s.starts_with("POST /20180401/metrics/actions/summarizeMetricsData")),
        "{seen:#?}"
    );
}
