//! The requests behind `backup` and `watch --auto-start`, against a local mock of the OCI API:
//! what is sent (method, path, body) and how the answers are read.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use oci_free_keeper_core::model::Instance;
use oci_free_keeper_core::{api, backup};
use oci_request_signer::{Client, Profile};
use serde_json::{Value, json};

/// One request as the mock saw it.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    body: Vec<u8>,
}

type Route = fn(&str, &str, &[u8]) -> (u16, Vec<(&'static str, &'static str)>, String);

fn mock(route: Route) -> (Client, Arc<Mutex<Vec<Seen>>>) {
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
            let (status, headers, resp_body) = route(&method, &path, &body);
            log.lock().unwrap().push(Seen { method, path, body });
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
    let profile = Profile {
        user: "ocid1.user.oc1..u".into(),
        fingerprint: "aa:bb".into(),
        tenancy: "ocid1.tenancy.oc1..t".into(),
        region: "eu-amsterdam-1".into(),
        key_file: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test-key.pem"),
        pass_phrase: None,
    };
    let client = Client::builder(&profile).endpoint(base).build().unwrap();
    (client, seen)
}

fn not_found(method: &str, path: &str) -> (u16, Vec<(&'static str, &'static str)>, String) {
    (
        404,
        vec![],
        format!(r#"{{"code":"NotFound","message":"no mock for {method} {path}"}}"#),
    )
}

fn instance(ad: Option<&str>) -> Instance {
    serde_json::from_value(json!({
        "id": "ocid1.instance.oc1..arm",
        "compartmentId": "ocid1.compartment.oc1..c1",
        "displayName": "arm",
        "shape": "VM.Standard.A1.Flex",
        "lifecycleState": "RUNNING",
        "availabilityDomain": ad,
    }))
    .unwrap()
}

#[test]
fn boot_volume_is_the_attached_one() {
    let (client, seen) = mock(
        |method, path, _| match (method, path.split('?').next().unwrap()) {
            ("GET", "/20160918/bootVolumeAttachments") => (
                200,
                vec![],
                r#"[{"bootVolumeId":"ocid1.bootvolume.oc1..old","lifecycleState":"DETACHED"},
                {"bootVolumeId":"ocid1.bootvolume.oc1..now","lifecycleState":"ATTACHED"}]"#
                    .into(),
            ),
            _ => not_found(method, path),
        },
    );
    let id = api::boot_volume_of(
        &client,
        "eu-amsterdam-1",
        &instance(Some("Abcd:EU-AMSTERDAM-1-AD-1")),
    )
    .unwrap();
    assert_eq!(id.as_deref(), Some("ocid1.bootvolume.oc1..now"));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    // Listed by availability domain, compartment and instance, all URL-encoded.
    assert!(
        seen[0]
            .path
            .contains("availabilityDomain=Abcd%3AEU-AMSTERDAM-1-AD-1")
            && seen[0]
                .path
                .contains("compartmentId=ocid1.compartment.oc1..c1")
            && seen[0].path.contains("instanceId=ocid1.instance.oc1..arm"),
        "{seen:#?}"
    );
}

#[test]
fn no_availability_domain_no_request() {
    let (client, seen) = mock(|method, path, _| not_found(method, path));
    assert_eq!(
        api::boot_volume_of(&client, "eu-amsterdam-1", &instance(None)).unwrap(),
        None
    );
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn backups_of_every_compartment_and_live_block_backups() {
    let (client, _) = mock(|method, path, _| {
        let p = path.split('?').next().unwrap();
        match (method, p) {
            ("GET", "/20160918/bootVolumeBackups") if path.contains("page=2") => (
                200,
                vec![],
                r#"[{"id":"b2","bootVolumeId":"bv1","lifecycleState":"AVAILABLE",
                     "timeCreated":"2026-11-02T03:00:00.000Z"}]"#
                    .into(),
            ),
            ("GET", "/20160918/bootVolumeBackups") if path.contains("..c1") => (
                200,
                vec![("opc-next-page", "2")],
                r#"[{"id":"b1","displayName":"arm 2026-11-01 (oci-free-keeper)","bootVolumeId":"bv1",
                     "lifecycleState":"AVAILABLE","timeCreated":"2026-11-01T03:00:00.000Z",
                     "freeformTags":{"managed-by":"oci-free-keeper"}}]"#
                    .into(),
            ),
            ("GET", "/20160918/bootVolumeBackups") => (200, vec![], "[]".into()),
            ("GET", "/20160918/volumeBackups") if path.contains("..c2") => (
                200,
                vec![],
                r#"[{"lifecycleState":"AVAILABLE"},{"lifecycleState":"TERMINATED"},
                    {"lifecycleState":"CREATING"}]"#
                    .into(),
            ),
            ("GET", "/20160918/volumeBackups") => (200, vec![], "[]".into()),
            _ => not_found(method, path),
        }
    });
    let (boot, block) = api::backups(
        &client,
        "eu-amsterdam-1",
        &[
            "ocid1.compartment.oc1..c1".into(),
            "ocid1.compartment.oc1..c2".into(),
        ],
    )
    .unwrap();
    let ids: Vec<&str> = boot.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["b1", "b2"], "both pages of the first compartment");
    assert!(boot[0].ours() && !boot[1].ours());
    assert_eq!(block, 2, "terminated block backups do not count");
}

#[test]
fn create_backup_sends_a_full_tagged_backup() {
    let (client, seen) = mock(|method, path, _| match (method, path) {
        ("POST", "/20160918/bootVolumeBackups") => (
            200,
            vec![],
            r#"{"id":"ocid1.bootvolumebackup.oc1..new","lifecycleState":"CREATING",
                "freeformTags":{"managed-by":"oci-free-keeper"}}"#
                .into(),
        ),
        _ => not_found(method, path),
    });
    let b = api::create_backup(
        &client,
        "eu-amsterdam-1",
        "bv1",
        "arm 2026-11-11 (oci-free-keeper)",
    )
    .unwrap();
    assert_eq!(b.lifecycle_state, "CREATING");
    assert!(b.ours());
    let seen = seen.lock().unwrap();
    let body: Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(
        body,
        json!({
            "bootVolumeId": "bv1",
            "displayName": "arm 2026-11-11 (oci-free-keeper)",
            "type": "FULL",
            "freeformTags": { backup::TAG_KEY: backup::TAG_VALUE },
        })
    );
}

#[test]
fn get_and_delete_a_backup() {
    let (client, seen) = mock(|method, path, _| match (method, path) {
        ("GET", "/20160918/bootVolumeBackups/ocid1.bootvolumebackup.oc1..x") => (
            200,
            vec![],
            r#"{"id":"ocid1.bootvolumebackup.oc1..x","lifecycleState":"AVAILABLE"}"#.into(),
        ),
        ("DELETE", "/20160918/bootVolumeBackups/ocid1.bootvolumebackup.oc1..x") => {
            (204, vec![], String::new())
        }
        _ => not_found(method, path),
    });
    let b = api::get_backup(&client, "eu-amsterdam-1", "ocid1.bootvolumebackup.oc1..x").unwrap();
    assert_eq!(b.lifecycle_state, "AVAILABLE");
    api::delete_backup(&client, "eu-amsterdam-1", "ocid1.bootvolumebackup.oc1..x").unwrap();
    let methods: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.method.clone())
        .collect();
    assert_eq!(methods, ["GET", "DELETE"]);
}

#[test]
fn start_sends_an_empty_body() {
    // OCI refuses `{}` with 400 Invalid typeId; the mock does the same.
    let (client, seen) = mock(|method, path, body| match (method, path) {
        ("POST", "/20160918/instances/ocid1.instance.oc1..arm?action=START") if body.is_empty() => {
            (200, vec![], r#"{"lifecycleState":"STARTING"}"#.into())
        }
        ("POST", _) => (
            400,
            vec![],
            r#"{"code":"InvalidParameter","message":"Invalid typeId"}"#.into(),
        ),
        _ => not_found(method, path),
    });
    assert_eq!(
        api::start_instance(&client, "eu-amsterdam-1", "ocid1.instance.oc1..arm").unwrap(),
        "STARTING"
    );
    assert!(seen.lock().unwrap()[0].body.is_empty());
}
