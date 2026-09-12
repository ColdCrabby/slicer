#!/usr/bin/env python3
"""Minimal fake Moonraker (Klipper) host for exercising printer detection.

The slicer's setup wizard builds a whole printer profile out of what a Klipper
machine says about itself, so this script has to answer the same four endpoints
a real Moonraker would, and answer them *differently* per query — the wizard
asks for one object at a time so a huge config can fail without taking the bed
size down with it:

- ``/printer/info`` — identity
- ``/printer/objects/query?toolhead`` — reachable axis limits
- ``/printer/objects/list`` — which modules and macros exist
- ``/printer/objects/query?bed_mesh`` — saved mesh profiles
- ``/printer/objects/query?configfile`` — the parsed ``printer.cfg``

Usage::

    python3 tools/mock-printer/moonraker.py                  # Voron-ish, :7199
    python3 tools/mock-printer/moonraker.py --preset bare     # nothing optional
    python3 tools/mock-printer/moonraker.py --preset ambiguous  # both macros
    python3 tools/mock-printer/moonraker.py --kinematics delta --name my-delta
    python3 tools/mock-printer/moonraker.py --drop configfile  # simulate a timeout

Then point the wizard's "Detect" field at ``http://127.0.0.1:7199``.
"""

import argparse
import json
from http.server import BaseHTTPRequestHandler, HTTPServer

# Which optional modules and macros each preset advertises. The presets exist to
# reach the three interesting wizard outcomes: nothing to ask, one settled macro
# convention, and a genuinely ambiguous one.
PRESETS = {
    "full": [
        "exclude_object",
        "bed_mesh",
        "quad_gantry_level",
        "firmware_retraction",
        "fan_generic rscs",
        "gcode_macro START_PRINT",
        "gcode_macro END_PRINT",
    ],
    "standard": [
        "exclude_object",
        "bed_mesh",
        "gcode_macro PRINT_START",
        "gcode_macro PRINT_END",
    ],
    "ambiguous": [
        "bed_mesh",
        "gcode_macro PRINT_START",
        "gcode_macro START_PRINT",
    ],
    "bare": [],
}


def build_payloads(args):
    """Assemble the canned reply for each probed endpoint."""
    modules = PRESETS[args.preset]

    settings = {
        "printer": {
            "kinematics": args.kinematics,
            "max_velocity": args.max_velocity,
            "max_accel": args.max_accel,
            "square_corner_velocity": 8.0,
        },
        "extruder": {
            "nozzle_diameter": args.nozzle,
            "filament_diameter": 1.75,
            "pressure_advance": 0.032,
        },
    }
    if "firmware_retraction" in modules:
        settings["firmware_retraction"] = {
            "retract_length": 0.8,
            "retract_speed": 35.0,
            "unretract_extra_length": 0.0,
        }
    # Every module is also a config section on a real host.
    for module in modules:
        settings.setdefault(module.lower(), {})

    return {
        "info": {
            "result": {
                "state": "ready",
                "hostname": args.name,
                "software_version": "v0.12.0-mock",
            }
        },
        "toolhead": {
            "result": {
                "status": {
                    "toolhead": {
                        "axis_maximum": [args.width, args.depth, args.height, 0],
                        "axis_minimum": [0, 0, 0, 0],
                    }
                }
            }
        },
        "list": {"result": {"objects": ["webhooks", "configfile", "toolhead", "extruder", *modules]}},
        "bed_mesh": {
            "result": {
                "status": {
                    "bed_mesh": {"profiles": {"default": {}, "abs": {}}}
                    if "bed_mesh" in modules
                    else {}
                }
            }
        },
        "configfile": {"result": {"status": {"configfile": {"settings": settings}}}},
    }


def make_handler(payloads, dropped):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, fmt, *args):
            print(f"  → {fmt % args}")

        def _send(self, body):
            data = json.dumps(body).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def _stage(self):
            """Which canned reply this request wants, or None for a 404."""
            if self.path.startswith("/printer/info"):
                return "info"
            if self.path.startswith("/printer/objects/list"):
                return "list"
            if self.path.startswith("/printer/objects/query"):
                query = self.path.partition("?")[2]
                for stage in ("toolhead", "bed_mesh", "configfile"):
                    if stage in query:
                        return stage
            return None

        def do_GET(self):
            stage = self._stage()
            # `--drop` stands in for the stage that times out or 500s on a real
            # macro-heavy host; detection is meant to survive losing any of them
            # but `/printer/info`.
            if stage is None or stage in dropped:
                self.send_response(404)
                self.end_headers()
                return
            self._send(payloads[stage])

    return Handler


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=7199)
    parser.add_argument("--name", default="voron-2.4-test", help="reported hostname")
    parser.add_argument(
        "--preset",
        default="full",
        choices=sorted(PRESETS),
        help="which optional modules and start macros to advertise",
    )
    parser.add_argument(
        "--kinematics",
        default="corexy",
        help="e.g. cartesian, corexy, delta (delta => circular/center-origin)",
    )
    parser.add_argument("--width", type=float, default=350.0)
    parser.add_argument("--depth", type=float, default=350.0)
    parser.add_argument("--height", type=float, default=340.0)
    parser.add_argument("--nozzle", type=float, default=0.4)
    parser.add_argument("--max-velocity", type=float, default=300.0)
    parser.add_argument("--max-accel", type=float, default=6000.0)
    parser.add_argument(
        "--drop",
        action="append",
        default=[],
        metavar="STAGE",
        help="answer 404 for this stage (toolhead, list, bed_mesh, configfile)",
    )
    args = parser.parse_args()

    payloads = build_payloads(args)
    server = HTTPServer((args.host, args.port), make_handler(payloads, set(args.drop)))
    print(
        f"Mock Moonraker on http://{args.host}:{args.port} — "
        f"{args.name} ({args.kinematics}, preset {args.preset})"
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        server.server_close()


if __name__ == "__main__":
    main()
