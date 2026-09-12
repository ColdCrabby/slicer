# Mock printer hosts

Tiny fake network-printer servers for exercising the slicer's **printer
detection** flow (the "Detect" field in the add-printer wizard) without a real
machine on the LAN.

## Why

The wizard probes a URL server-side and builds a whole printer profile out of
what the machine says about itself. For Klipper it asks, in this order:

| Endpoint | Answers |
| --- | --- |
| `GET /printer/info` | identity (`state`, `hostname`) |
| `GET /printer/objects/query?toolhead` | build volume, from the axis limits |
| `GET /printer/objects/list` | which modules and start macros exist |
| `GET /printer/objects/query?bed_mesh` | saved mesh profiles |
| `GET /printer/objects/query?configfile` | the configured values themselves |

The script answers each one separately, the way a real Moonraker does — which
matters, because the wizard is built to survive losing any of them but the
first.

## Moonraker (Klipper)

```bash
# Voron-ish CoreXY 350³ with every optional module, on :7199
python3 tools/mock-printer/moonraker.py

# A machine with nothing optional — the wizard has to ask about macros
python3 tools/mock-printer/moonraker.py --preset bare

# Both macro conventions defined — the one case the config can't settle
python3 tools/mock-printer/moonraker.py --preset ambiguous

# A delta → detected as circular bed, center origin
python3 tools/mock-printer/moonraker.py --kinematics delta --name my-delta

# Custom bed / nozzle / port
python3 tools/mock-printer/moonraker.py --port 8080 --width 250 --depth 210 --nozzle 0.6

# Pretend the big config payload times out — detection should degrade, not fail
python3 tools/mock-printer/moonraker.py --drop configfile
```

`--preset` picks which optional modules and macros the fake host advertises:
`full` (the default), `standard` (`PRINT_START`), `ambiguous` (both
conventions), `bare` (none).

Then enter `http://127.0.0.1:7199` in the wizard's **Detect** field and run it.

Stdlib only — no dependencies. Stop with Ctrl-C.
