# Self-hosting

Run the server, and everyone on your network gets the same slicer at the same
address — with a shared profile library, shared plate history, and printers that
actually connect.

## What you need

- **Rust**, via [rustup](https://rustup.rs/). The Homebrew `rust` package will
  not work — it can't add the WebAssembly target.
- **Node.js 20+** and **pnpm 9+**.
- The `wasm32-unknown-unknown` target and a matching `wasm-bindgen-cli`.

Full prerequisites, including the exact `wasm-bindgen` pinning command, are in
[Setup](/guide/setup).

## Build and run

```bash
pnpm install        # workspace dependencies
pnpm run hydrate    # WebAssembly bindings + generated types
pnpm run ui:build   # the web interface
cargo build --release
```

Then start it:

```bash
./target/release/slicer-engine serve
```

It listens on `http://localhost:5201/`.

::: warning Run the steps in order
`hydrate` populates generated bindings that `ui:build` needs, and `ui:build`
produces the directory `serve` looks for. Skipping either gives you a confusing
error one step later.
:::

## Exposing it on the network

By default the server binds `127.0.0.1` — local only. To let other machines
reach it:

```bash
slicer-engine serve --host 0.0.0.0 --port 5201
```

Or in `slicer.toml`:

```toml
[server]
host = "0.0.0.0"
port = 5201
```

| Flag | Default | What it does |
| --- | --- | --- |
| `--host` | `127.0.0.1` | Interface to bind. `0.0.0.0` for all. |
| `--port` | `5201` | TCP port |
| `--ui-dir` | `./ui/dist/slicer-ui/browser` | Where the built interface lives |
| `--work-dir` | system temp | Where uploaded models and G-code are staged |

`cors_origins` in `slicer.toml` restricts which web origins may call the API.
Empty means no restriction.

::: danger No authentication
The server has no accounts and no login. Anyone who can reach the port can
slice, read history, and use configured printers. Keep it on a trusted network,
or put a reverse proxy with authentication in front of it.
:::

## Running it as a service

There's no packaged service unit, but the binary is a normal long-running
process. A minimal systemd unit:

```ini
[Unit]
Description=Cold Crabby slicer
After=network.target

[Service]
ExecStart=/opt/coldcrabby/slicer-engine serve --host 0.0.0.0
WorkingDirectory=/opt/coldcrabby
Restart=on-failure
User=coldcrabby

[Install]
WantedBy=multi-user.target
```

Point `WorkingDirectory` at the directory holding the built `ui/` output, or
pass `--ui-dir` explicitly.

## Behind a reverse proxy

The interface talks to the server over both HTTP and a WebSocket. Your proxy
must forward WebSocket upgrades on `/ws`, or slicing will appear to hang with no
progress.

```nginx
location / {
    proxy_pass http://127.0.0.1:5201;
    proxy_http_version 1.1;
    proxy_set_header Upgrade    $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_set_header Host       $host;
}
```

Model uploads are large — raise `client_max_body_size` to at least 500 MB.

## What lives where

| | Where | Note |
| --- | --- | --- |
| Config | `~/.config/slicer-engine/slicer.toml` | Created on first run |
| Profile library | `profiles.toml`, beside the config | Printers, filaments, processes, labels |
| Slice history & G-code cache | `slicer.db` in the work directory | SQLite |
| Uploads & output | The work directory | `<system temp>/slicer-engine` by default |

::: warning Set `--work-dir` for anything long-lived
History, cached G-code and uploaded models all live in the work directory, and
it defaults to system temp. On a machine that clears temp on reboot, your slice
history goes with it. Point it somewhere real:

```toml
[server]
work_dir = "/var/lib/coldcrabby"
```
:::

Back up the config directory and the work directory and you've backed up the
instance.

## Updating

Pull, rebuild, restart:

```bash
git pull
pnpm install && pnpm run hydrate && pnpm run ui:build
cargo build --release
```

Cached G-code is keyed by engine version, so an upgrade invalidates it
automatically — you'll never get output from the previous version by accident.

## Checking it works

```bash
curl -f http://localhost:5201/ && echo OK
```

Then open it in a browser, slice the 3DBenchy demo, and confirm the preview
appears. If a printer is configured, check its status dot goes green — that
confirms the server's network path to the printer, which is the main reason to
self-host in the first place.

## Other ways to run it

- **[Desktop app](/guide/building)** — bundles engine and interface into one
  native application. No server.
- **Browser build** — compiles the whole engine to WebAssembly and needs no
  backend at all. Useful for a static deployment where nothing should touch a
  server. Note that printer connections won't work from it.
