# rqbit

This is an actively developed fork of [ikatson/rqbit](https://github.com/ikatson/rqbit): a Rust BitTorrent client with a CLI, native HTTP API, web UI, and Tauri desktop app. `librqbit` can also be embedded as a library.

Alongside the upstream client capabilities, this fork adds:

- an opt-in, authenticated qBittorrent Web API compatibility layer;
- persistent download categories and automation-oriented torrent metadata;
- web UI login/logout and category management;
- JSON or PostgreSQL session persistence; and
- hardened HTTP behavior, including safer logging and authentication requirements for writable non-loopback listeners.

This repository is not an official upstream release channel. Fork-specific changes may not be present in crates.io packages, upstream releases, or `ikatson/rqbit` Docker images.

## Quick start

Build this checkout first (see [Build and install](#build-and-install)), then start a persistent server:

```sh
rqbit server start ~/Downloads
```

The native API and web UI are available at <http://127.0.0.1:3030/> and <http://127.0.0.1:3030/web/>. Add a torrent with a magnet URI, an HTTP(S) `.torrent` URL, or a local torrent file:

```sh
rqbit download -o ~/Downloads 'magnet:?xt=urn:btih:...'
rqbit download -o ~/Downloads https://example.org/file.torrent
rqbit download -o ~/Downloads /path/to/file.torrent
```

For a long-running server, submit torrents through the web UI or native API. Use `rqbit --help` and `rqbit server start --help` as the authoritative CLI reference.

## Web UI and authentication

The default API listener is loopback-only. When exposing the writable API outside the host, configure Basic authentication and terminate TLS in a reverse proxy or another trusted encrypted access layer:

```sh
RQBIT_HTTP_BASIC_AUTH_USERPASS='username:password' \
  rqbit --http-api-listen-addr 0.0.0.0:3030 server start /srv/torrents
```

Basic authentication protects access but does not encrypt traffic. Do not expose the HTTP listener directly to an untrusted network.

The web UI supports authenticated login and logout. It provides torrent management, streaming, log viewing, settings, category management, and the server's public IP address. After the UI is authenticated, rqbit obtains that address through an outbound HTTPS request to `api64.ipify.org`; if the lookup is unavailable, the UI continues without displaying it.

## qBittorrent Web API compatibility

Enable the compatibility routes with credentials:

```sh
RQBIT_HTTP_BASIC_AUTH_USERPASS='client:change-me' \
RQBIT_QBITTORRENT_API_ENABLE=true \
RQBIT_HTTP_API_LISTEN_ADDR=0.0.0.0:3030 \
  rqbit server start /srv/torrents
```

Connect clients using the qBittorrent Web API with the same host, port, and credentials. Categories retain rqbit's global download directory rather than moving content.

Torrent-add boolean fields (`paused`, `sequentialDownload`, and `firstLastPiecePrio`) accept case-insensitive `true`/`false` and `1`/`0` in both URL-encoded and multipart forms, including Radarr's `True`/`False` values. Invalid boolean values are rejected.

This is not a full qBittorrent replacement. Queue ordering, sequential downloading, first/last-piece priority, and inactive-seeding-time limits are intentionally unsupported and fail explicitly.

For a production-oriented Gluetun, Sonarr, and Radarr configuration with shared `/data` paths and hardlinks, use the [automation stack deployment guide](docker/compose-examples/automation-stack.md).

## Persistence

`rqbit server start` persists its state by default. Use a chosen JSON persistence directory when deploying a server:

```sh
rqbit server start \
  --persistence-location /var/lib/rqbit/session \
  /srv/torrents
```

PostgreSQL is also supported. Pass a `postgres://` connection string as the persistence location:

```sh
rqbit server start \
  --persistence-location 'postgres://user:password@db.example/rqbit' \
  /srv/torrents
```

Use `--fastresume` to skip checksumming on restart when that trade-off is appropriate. Do not disable persistence for an automation server unless losing its managed-torrent state is intentional.

## Streaming and LAN features

rqbit can stream torrent files while prioritizing the pieces being read, including seeking through HTTP range requests. A stream URL has this form:

```text
http://HOST:3030/torrents/TORRENT_ID/stream/FILE_ID
```

It can also expose managed torrents through a UPnP media server and advertise the HTTP API over mDNS/DNS-SD:

```sh
RQBIT_HTTP_BASIC_AUTH_USERPASS='username:password' \
  rqbit --enable-upnp-server --enable-mdns \
  --http-api-listen-addr 0.0.0.0:3030 server start /srv/torrents
```

With mDNS enabled, the UI may be reachable on the LAN at `http://rqbit.local:3030/web/`. Use a non-loopback listener and credentials for either LAN feature.

## Build and install

The supported way to use fork-specific functionality is to build this repository. Install a current Rust toolchain and Node.js, then:

```sh
git clone https://github.com/Mallone2000/rqbit.git
cd rqbit
cd crates/librqbit/webui && npm ci && cd ../../..
cargo build --release
```

The default build embeds the web UI, so npm is required. The resulting binary is `target/release/rqbit`.

For development:

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --all-targets
```

See [DEV-GUIDE.md](DEV-GUIDE.md) for local server and web UI workflows.

## Desktop app

The desktop app wraps the same web UI in Tauri. Build it from this checkout after installing Rust and Node.js:

```sh
cd desktop
npm ci
cargo tauri build
```

## Docker

The repository publishes `ghcr.io/mallone2000/rqbit:latest` from successful
builds of `main`. The image supports `linux/amd64`, `linux/arm64`, and
`linux/arm/v7`. Start the included example with:

```sh
docker compose -f docker/compose-examples/server.yaml pull
RQBIT_USER=admin RQBIT_PASSWORD='change-me' \
  docker compose -f docker/compose-examples/server.yaml up -d
```

After the first workflow publish, set the GHCR package visibility to **Public**
once so Docker hosts can pull it without registry credentials. Subsequent
successful `main` builds update the same tag automatically, and the example's
`pull_policy: always` checks for that update whenever it starts.

The rolling `latest` tag is convenient for tracking `main`; pin a reviewed
digest when deployments require controlled updates. The [automation stack
deployment guide](docker/compose-examples/automation-stack.md) documents the
Gluetun topology and local-build fallback. Do not substitute the upstream
`ikatson/rqbit` image because it may not contain this fork's compatibility
features.

## Native HTTP API

The API root describes the routes implemented by the running binary:

```sh
curl -s http://127.0.0.1:3030/
```

Common native operations include listing torrents, adding a magnet or `.torrent`, pausing or resuming a torrent, selecting files, accessing Prometheus metrics, and streaming files. For example:

```sh
curl -d 'magnet:?xt=urn:btih:...' http://127.0.0.1:3030/torrents
curl http://127.0.0.1:3030/torrents
curl http://127.0.0.1:3030/metrics
```

When authentication is enabled, supply credentials with `curl -u username:password`. The qBittorrent-compatible routes are available only when `RQBIT_QBITTORRENT_API_ENABLE=true` is configured.

## Other useful capabilities

- DHT, local peer discovery, IPv4/IPv6 dual-stack listeners, and tracker support.
- Optional experimental uTP listener support.
- SOCKS5 proxy support: `rqbit --socks-url socks5://user:password@host:port ...`.
- UPnP port forwarding and a UPnP media server.
- Watched folders: `rqbit server start --watch-folder /path/to/torrents /download/path`.
- Prometheus metrics at `/metrics` and per-torrent peer metrics.
- Shell completions, for example: `eval "$(rqbit completions bash)"`.
- Systemd socket activation via the units in [systemd](systemd).

## Code organization

- `crates/rqbit` — CLI binary and server configuration.
- `crates/librqbit` — torrent session, HTTP APIs, storage, persistence, and web UI.
- `crates/librqbit_core` — shared torrent types and utilities.
- `crates/dht`, `crates/tracker_comms`, `crates/peer_binary_protocol` — protocol implementations.
- `crates/upnp` and `crates/upnp-serve` — port forwarding and media-server support.
- `desktop` — Tauri desktop application.
- `docker` — container build files and Compose examples.

## Upstream and license

rqbit was created by Igor Katson and is licensed under Apache-2.0. This fork retains that license and upstream attribution; see [LICENSE](LICENSE). Upstream releases and documentation remain available at [ikatson/rqbit](https://github.com/ikatson/rqbit).
