# Mock printer hosts

Tiny fake network-printer servers for exercising the slicer's **printer
detection** flow (the "Detect" field in the add-printer wizard) without a real
machine on the LAN.

## Why

The wizard probes a URL server-side and prefills a printer profile from what it
finds. For Klipper it reads:

- `GET /printer/info` — identity (`state`, `hostname`)
- `GET /printer/objects/query?configfile&toolhead` — bed volume (toolhead axis
  limits), kinematics (delta ⇒ circular / center-origin), and nozzle diameter

These scripts answer exactly those endpoints with canned JSON, so you can click
through detection end to end.

## Moonraker (Klipper)

```bash
# Cartesian 350³ Voron on :7199 (defaults)
python3 tools/mock-printer/moonraker.py

# A delta → detected as circular bed, center origin
python3 tools/mock-printer/moonraker.py --kinematics delta --name my-delta

# Custom bed / nozzle / port
python3 tools/mock-printer/moonraker.py --port 8080 --width 250 --depth 210 --nozzle 0.6
```

Then enter `http://127.0.0.1:7199` in the wizard's **Detect** field and run it.

Stdlib only — no dependencies. Stop with Ctrl-C.
