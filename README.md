<div align="center">

<img src="https://raw.githubusercontent.com/ColdCrabby/slicer/main/ui/public/logo_hero.png" alt="Cold Crabby mascot - a crab hugging an ice cube" width="200" />

# Cold Crabby

**Slice your 3D models everywhere - desktop, tablet, or your own server.**

The web version runs fully in the browser, so you can slice on an iPad or any device that can't run a desktop slicer.

🌐 **[Try the online slicer](https://slicer.maxscopp.de/)** → no install, no account.

[![Tests](https://img.shields.io/github/actions/workflow/status/ColdCrabby/slicer/test-results.yml?branch=main&label=tests)](https://github.com/ColdCrabby/slicer/actions/workflows/test-results.yml)
[![Quality](https://img.shields.io/github/actions/workflow/status/ColdCrabby/slicer/quality.yml?branch=main&label=quality)](https://github.com/ColdCrabby/slicer/actions/workflows/quality.yml)
[![Frontend CI](https://img.shields.io/github/actions/workflow/status/ColdCrabby/slicer/ui-ci.yml?branch=main&label=frontend)](https://github.com/ColdCrabby/slicer/actions/workflows/ui-ci.yml)
[![Security](https://img.shields.io/github/actions/workflow/status/ColdCrabby/slicer/security.yml?branch=main&label=security)](https://github.com/ColdCrabby/slicer/actions/workflows/security.yml)
[![Rust lines](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2FColdCrabby%2Fslicer%2Fbadges%2Frust.json)](https://github.com/ColdCrabby/slicer/actions/workflows/sloc.yml)
[![TypeScript lines](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2FColdCrabby%2Fslicer%2Fbadges%2Ftypescript.json)](https://github.com/ColdCrabby/slicer/actions/workflows/sloc.yml)

</div>

---

Drop in an STL, OBJ, or 3MF and get print-ready G-code. One engine, wherever you are:

|                    | Where it runs                                        | Setup                                                    |
| ------------------ | ---------------------------------------------------- | -------------------------------------------------------- |
| 🌐 **Web**         | In any browser - desktop, tablet, iPad               | None - [just open the link](https://slicer.maxscopp.de/) |
| 🖥️ **Desktop**     | Native app, runs entirely on your machine            | [Set it up](SETUP.md#desktop-app)                        |
| 📱 **iPad**        | The same app, with touch and pen                     | [Set it up](SETUP.md#ipad--ios-app)                      |
| ☁️ **Self-hosted** | Host it yourself, share with your team               | [Set it up](SETUP.md#self-hosted-web-ui)                 |

Same slicing engine everywhere, so the G-code is identical no matter where you run it. In the browser, your files never leave your machine.

The interface adapts to whatever you're on - touch and pen gestures on a tablet, a native window that feels at home on desktop - so it always fits the device instead of fighting it.

**What you get:** multi-object build plates with auto-arrange and per-face orientation · variable-width perimeters (Arachne) or classic fixed-offset walls · rectilinear, grid, honeycomb, gyroid and TPMS-D infill · supports, brims, rafts and ironing · spiral vase mode · Marlin and Klipper G-code with custom start/end scripts · a full toolpath preview you can colour by role, speed, flow or temperature · cancel-one-object and sequential printing · upload and start a print on Klipper machines without leaving the app.

---

## Get started

**If you want to print something**

- 🌐 **Slice now, nothing to install** → [slicer.maxscopp.de](https://slicer.maxscopp.de/)
- 📘 **How to use it** — the interface, settings, printers, shortcuts → [Getting started](https://slicer.maxscopp.de/docs/use/)
- 🖥️ **Run it on your own machine** → [SETUP.md](SETUP.md)

**If you want to run it for other people**

- 🏢 **Teams & businesses** — self-hosting, shared profiles, automation, privacy → [For teams](https://slicer.maxscopp.de/docs/teams/)

**If you want to work on it**

- 🧱 **Build from source** → [BUILDING.md](BUILDING.md)
- 🧑‍💻 **Develop & contribute** → [DEVELOPMENT.md](DEVELOPMENT.md) · [CONTRIBUTING.md](CONTRIBUTING.md)
- 🗺️ **Architecture** → [ARCHITECTURE.md](ARCHITECTURE.md)

---

## About the project

For me, Cold Crabby is an experiment in **agentic engineering**: I set out to build a genuinely hard, domain-heavy product where AI writes most of the code - but under real engineering discipline, not vibes.

A slicer is deep, unforgiving territory: computational geometry, numerical precision, decades of hard-won tricks from established slicers. I couldn't vibe-code my way through that, and honestly I don't hold every detail of it in my own head. What makes it work is the engineering I put _around_ the AI - clear guardrails, tight contracts, and reviewing every change - so the model is guided toward correct results instead of plausible-looking ones.

At the same time, AI is the only reason this exists at all. Reimplementing proven approaches from scratch in Rust would have taken me months just to reach a working baseline. With agentic tooling that baseline arrived fast, and my job shifted to steering, reviewing, and hardening.

Take the [Arachne wall generator](src/walls/arachne/) - variable-width beads from a medial axis, one of the genuinely hard parts of any slicer. It didn't come from a single prompt. It cost me weekends of plans, questionnaires, and re-implementations, reframing the problem again and again until it was scoped small enough for the AI to actually tackle. That scoping is where the magic happened: broken into the right pieces, even something this hard got built correctly.

So it's two things at once for me: a real product I want to use, and a running experiment in how far I can push AI on a large, complex codebase over the long haul - watching how it copes as the system grows, and how far I can guide genuinely hard problems home.

---

## License

All rights reserved until an official license is decided. No use, reproduction, modification, or distribution permitted without written authorization. TBD.

---

## Support

[Issues](https://github.com/max-scopp/slicer-engine/issues) · [Discussions](https://github.com/max-scopp/slicer-engine/discussions) · [Contributing](CONTRIBUTING.md) · [Documentation site](https://slicer.maxscopp.de/docs/)
