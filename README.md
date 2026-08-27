<div align="center">

<img src="ui/public/logo_still@2x.png" alt="Cold Crabby mascot - a crab hugging an ice cube" width="200" />

# Cold Crabby

**Slice your 3D models everywhere - desktop, tablet, or your own server.**

The web version runs fully in the browser, so you can slice on an iPad or any device that can't run a desktop slicer.

🌐 **[Try the online slicer](https://slicer.maxscopp.de/)** → no install, no account.

[![Multi-Platform Build](https://github.com/ColdCrabby/slicer/actions/workflows/build.yml/badge.svg)](https://github.com/ColdCrabby/slicer/actions/workflows/build.yml)
[![Frontend CI](https://github.com/ColdCrabby/slicer/actions/workflows/ui-ci.yml/badge.svg)](https://github.com/ColdCrabby/slicer/actions/workflows/ui-ci.yml)
[![Security](https://github.com/ColdCrabby/slicer/actions/workflows/security.yml/badge.svg)](https://github.com/ColdCrabby/slicer/actions/workflows/security.yml)
[![SLOC](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2FColdCrabby%2Fslicer%2Fmain%2F.github%2Fbadges%2Fsloc.json)](https://github.com/ColdCrabby/slicer/actions/workflows/sloc.yml)

</div>

---

Drop in an STL, OBJ, or 3MF and get print-ready G-code. One engine, wherever you are:

|                    | Where it runs                                        | Setup                                                    |
| ------------------ | ---------------------------------------------------- | -------------------------------------------------------- |
| 🌐 **Web**         | In any browser - desktop, tablet, iPad               | None - [just open the link](https://slicer.maxscopp.de/) |
| 🖥️ **Desktop**     | Native app, runs entirely on your machine            | [Set it up](SETUP.md#desktop-app)                        |
| ☁️ **Self-hosted** | Host it yourself, share with your team               | [Set it up](SETUP.md#self-hosted-web-ui)                 |

Same slicing engine everywhere, so the G-code is identical no matter where you run it. In the browser, your files never leave your machine.

The interface adapts to whatever you're on - touch and pen gestures on a tablet, a native window that feels at home on desktop - so it always fits the device instead of fighting it.

---

## Get started

- 🌐 **Slice now, nothing to install** → [slicer.maxscopp.de](https://slicer.maxscopp.de/)
- 🛠️ **Run it yourself** - desktop, self-hosted, or the browser build → [SETUP.md](SETUP.md)
- 🧱 **Build from source** → [BUILDING.md](BUILDING.md)
- 🧑‍💻 **Develop & contribute** → [DEVELOPMENT.md](DEVELOPMENT.md) · [CONTRIBUTING.md](CONTRIBUTING.md)
- 📖 **Full docs** - architecture, module guides, contributor docs → [slicer.maxscopp.de/docs](https://slicer.maxscopp.de/docs/)

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
