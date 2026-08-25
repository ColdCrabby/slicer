#!/usr/bin/env python3
"""Minimal fake Moonraker (Klipper) host for exercising printer detection.

The slicer's setup wizard probes a URL and prefills a printer profile from the
Moonraker endpoints it finds (``/printer/info`` for identity, then
``/printer/objects/query?configfile&toolhead`` for bed volume, kinematics and
nozzle diameter). Standing up a real Klipper host just to click through that
flow is overkill, so this script answers those two endpoints with canned JSON.

Usage::

    python3 tools/mock-printer/moonraker.py                 # cartesian, :7199
    python3 tools/mock-printer/moonraker.py --port 8080
    python3 tools/mock-printer/moonraker.py --kinematics delta --name my-delta

Then point the wizard's "Detect" field at ``http://127.0.0.1:7199``.
"""

import argparse
import json
from http.server import BaseHTTPRequestHandler, HTTPServer


def build_config(args):
    """Assemble the canned payloads for the two probed endpoints."""
    info = {"result": {"state": "ready", "hostname": args.name}}
    query = {
        "result": {
            "status": {
                "configfile": {
                    "settings": {
                        "printer": {"kinematics": args.kinematics},
                        "extruder": {"nozzle_diameter": args.nozzle},
                    }
                },
                "toolhead": {
                    "axis_maximum": [args.width, args.depth, args.height, 0],
                    "axis_minimum": [0, 0, 0, 0],
                },
            }
        }
    }
    return info, query


def make_handler(info, query):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass  # quiet by default

        def _send(self, body):
            data = json.dumps(body).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            if self.path.startswith("/printer/info"):
                self._send(info)
            elif self.path.startswith("/printer/objects/query"):
                self._send(query)
            else:
                self.send_response(404)
                self.end_headers()

    return Handler


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=7199)
    parser.add_argument("--name", default="voron-2.4-test", help="reported hostname")
    parser.add_argument(
        "--kinematics",
        default="cartesian",
        help="e.g. cartesian, corexy, delta (delta => circular/center-origin)",
    )
    parser.add_argument("--width", type=float, default=350.0)
    parser.add_argument("--depth", type=float, default=350.0)
    parser.add_argument("--height", type=float, default=340.0)
    parser.add_argument("--nozzle", type=float, default=0.4)
    args = parser.parse_args()

    info, query = build_config(args)
    server = HTTPServer((args.host, args.port), make_handler(info, query))
    print(f"Mock Moonraker on http://{args.host}:{args.port} — {args.name} ({args.kinematics})")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        server.server_close()


if __name__ == "__main__":
    main()
