#!/usr/bin/env python3
"""A tiny stand-in for the OCI API, for recording the README GIF.

Answers the calls `oci-free-keeper check` makes with two made-up instances:
arm-free-1 (A1, memory keeps it busy) and micro-1 (E2.1.Micro, idle).
It does not check signatures. Not for anything but demos.

    python3 assets/demo/mock-oci.py 8798
"""

import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

REGION = "eu-amsterdam-1"
T = "ocid1.tenancy.oc1..aaaademotenancy"
INSTANCES = [
    {"id": "ocid1.instance.oc1.eu-amsterdam-1.armfree1", "compartmentId": T, "displayName": "arm-free-1",
     "shape": "VM.Standard.A1.Flex", "lifecycleState": "RUNNING", "shapeConfig": {"ocpus": 4, "memoryInGBs": 24}},
    {"id": "ocid1.instance.oc1.eu-amsterdam-1.micro1", "compartmentId": T, "displayName": "micro-1",
     "shape": "VM.Standard.E2.1.Micro", "lifecycleState": "RUNNING", "shapeConfig": {"ocpus": 1, "memoryInGBs": 1}},
]
N = 2016  # 7 days of 5-minute points


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
            self.reply(200, INSTANCES)
        else:
            self.reply(404, {"code": "NotAuthorizedOrNotFound", "message": path})

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length", 0))) or b"{}")
        query = body.get("query", "")
        metric = query.split("[")[0]
        instance = query.split('resourceId = "')[-1].split('"')[0]
        points = [{"timestamp": "2026-11-10T00:00:00Z", "value": v} for v in series(metric, instance)]
        self.reply(200, [{"name": metric, "aggregatedDatapoints": points}])

    def log_message(self, *args):
        pass


HTTPServer(("127.0.0.1", int(sys.argv[1]) if len(sys.argv) > 1 else 8798), Handler).serve_forever()
