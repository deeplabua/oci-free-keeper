#!/usr/bin/env python3
"""A tiny stand-in for the OCI API, for recording the README GIF and trying `watch`.

Answers the calls `oci-free-keeper check` makes with two made-up instances:
arm-free-1 (A1, memory keeps it busy) and micro-1 (E2.1.Micro, idle).
It does not check signatures. Not for anything but demos.

    python3 assets/demo/mock-oci.py 8798
"""

import json
import os
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

REGION = "eu-amsterdam-1"
T = "ocid1.tenancy.oc1..aaaademotenancy"
INSTANCES = [
    {"id": "ocid1.instance.oc1.eu-amsterdam-1.armfree1", "compartmentId": T, "displayName": "arm-free-1",
     "shape": "VM.Standard.A1.Flex", "lifecycleState": "RUNNING", "shapeConfig": {"ocpus": 4, "memoryInGBs": 24},
     "availabilityDomain": "Abcd:EU-AMSTERDAM-1-AD-1"},
    {"id": "ocid1.instance.oc1.eu-amsterdam-1.micro1", "compartmentId": T, "displayName": "micro-1",
     "shape": "VM.Standard.E2.1.Micro", "lifecycleState": "RUNNING", "shapeConfig": {"ocpus": 1, "memoryInGBs": 1},
     "availabilityDomain": "Abcd:EU-AMSTERDAM-1-AD-1"},
]
N = 2016  # 7 days of 5-minute points

# Boot volume backups, kept while the mock runs: created AVAILABLE on the second look.
# MOCK_OTHER_BACKUPS=n adds n untagged backups that keeper must never delete.
BACKUPS = [
    {"id": f"ocid1.bootvolumebackup.oc1..other{k}", "displayName": f"manual {k}", "bootVolumeId": "x",
     "lifecycleState": "AVAILABLE", "timeCreated": "2026-01-0%dT00:00:00.000Z" % (k + 1), "freeformTags": {}}
    for k in range(int(os.environ.get("MOCK_OTHER_BACKUPS", "0")))
]
SEEN = {}
STARTED = set()


def boot_volume(instance_id):
    return instance_id.replace("instance", "bootvolume") + "boot"


def series(metric, instance):
    arm = "armfree1" in instance
    if metric == "CpuUtilization":
        return [(9.0 if i % 12 == 0 else 3.5) if arm else (4.0 if i % 20 == 0 else 1.2) for i in range(N)]
    if metric == "MemoryUtilization":
        return [31.0 + (i % 7) * 0.2 if arm else 46.0 for i in range(N)]
    # byte counters: ~2.5 Mbit/s on A1, ~40 kbit/s on the micro
    step = 94_000_000 if arm else 1_500_000
    return [float(step * i) for i in range(N)]


class Handler(BaseHTTPRequestHandler):
    def reply(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path = self.path.split("?")[0]
        if path.endswith("/regionSubscriptions"):
            self.reply(200, [{"regionName": REGION, "isHomeRegion": True}])
        elif path.endswith("/compartments"):
            self.reply(200, [])
        elif path.endswith("/instances"):
            # MOCK_GONE=micro-1 drops that instance, as if it was terminated or reclaimed.
            # MOCK_STOPPED=micro-1 reports it STOPPED until something sends it START.
            gone = os.environ.get("MOCK_GONE", "")
            stopped = os.environ.get("MOCK_STOPPED", "")
            self.reply(200, [dict(i, lifecycleState="STOPPED") if i["displayName"] == stopped and i["id"] not in STARTED else i
                             for i in INSTANCES if i["displayName"] != gone])
        elif path.endswith("/availabilityDomains"):
            self.reply(200, [{"name": "Abcd:EU-AMSTERDAM-1-AD-1"}])
        elif path.endswith("/bootVolumeAttachments"):
            iid = self.path.split("instanceId=")[-1].split("&")[0]
            self.reply(200, [{"instanceId": iid, "bootVolumeId": boot_volume(iid), "lifecycleState": "ATTACHED"}])
        elif path.endswith("/bootVolumeBackups"):
            self.reply(200, BACKUPS)
        elif "/bootVolumeBackups/" in path:
            b = next((b for b in BACKUPS if b["id"] == path.rsplit("/", 1)[1]), None)
            if b is None:
                return self.reply(404, {"code": "NotAuthorizedOrNotFound", "message": path})
            SEEN[b["id"]] = SEEN.get(b["id"], 0) + 1
            if b["lifecycleState"] == "CREATING" and SEEN[b["id"]] > 1:
                b["lifecycleState"] = "AVAILABLE"
            self.reply(200, b)
        elif path.endswith("/volumeBackups"):
            self.reply(200, [])
        elif path.endswith("/bootVolumes"):
            self.reply(200, [{"id": "ocid1.bootvolume.oc1.eu-amsterdam-1.micro1boot", "displayName": "micro-1 (Boot Volume)",
                              "sizeInGBs": 47, "lifecycleState": "AVAILABLE"}])
        else:
            self.reply(404, {"code": "NotAuthorizedOrNotFound", "message": path})

    def do_DELETE(self):
        for b in BACKUPS:
            if b["id"] == self.path.split("?")[0].rsplit("/", 1)[1]:
                b["lifecycleState"] = "TERMINATED"
                self.send_response(204)
                self.end_headers()
                return
        self.reply(404, {"code": "NotAuthorizedOrNotFound", "message": self.path})

    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("content-length", 0)))
        if "action=START" in self.path:
            if raw:  # OCI refuses a body here: 400 Invalid typeId
                return self.reply(400, {"code": "InvalidParameter", "message": "Invalid typeId provided"})
            STARTED.add(self.path.split("?")[0].rsplit("/", 1)[1])
            return self.reply(200, {"lifecycleState": "STARTING"})
        body = json.loads(raw or b"{}")
        if self.path.split("?")[0].endswith("/bootVolumeBackups"):
            b = {"id": f"ocid1.bootvolumebackup.oc1..keeper{len(BACKUPS)}", "displayName": body["displayName"],
                 "bootVolumeId": body["bootVolumeId"], "lifecycleState": "CREATING",
                 "timeCreated": "2026-11-10T%02d:00:00.000Z" % len(BACKUPS), "freeformTags": body.get("freeformTags", {})}
            BACKUPS.append(b)
            return self.reply(200, b)
        query = body.get("query", "")
        metric = query.split("[")[0]
        instance = query.split('resourceId = "')[-1].split('"')[0]
        points = [{"timestamp": "2026-11-10T00:00:00Z", "value": v} for v in series(metric, instance)]
        self.reply(200, [{"name": metric, "aggregatedDatapoints": points}])

    def log_message(self, *args):
        pass


HTTPServer(("127.0.0.1", int(sys.argv[1]) if len(sys.argv) > 1 else 8798), Handler).serve_forever()
