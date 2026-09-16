# rqbit with Sonarr and Radarr behind Gluetun

This deployment uses rqbit's opt-in qBittorrent Web API compatibility layer. Use
an rqbit image built from a release that contains this feature; do not use an
older image merely because its version matches the source tree.

When all three applications share Gluetun's network namespace, Sonarr and
Radarr must connect to rqbit at `127.0.0.1:3030`. Mount the same host data root
at exactly `/data` in each container so Completed Download Handling can
hardlink files rather than copy them.

Publish all three plaintext HTTP interfaces on the Docker host's loopback
address only. Do not configure these mappings to listen on every interface.
For remote access, keep the application ports private and place an authenticated
HTTPS reverse proxy or another trusted encrypted access layer in front of them.
rqbit refuses to start the qBittorrent compatibility API unless a non-empty
`RQBIT_HTTP_BASIC_AUTH_USERPASS` value is configured.

Use reviewed container image references pinned by digest (`name@sha256:...`).
Do not use a mutable `latest` fallback.

On Windows, build and export a deployable Linux image from the repository root:

```powershell
.\scripts\build-docker-image.ps1
```

The default output is `rqbit:test` plus a timestamped
`target/rqbit-linux-amd64-*.tar` archive. Use `-Platform linux/arm64` for an
ARM64 server, or `-Image` and `-Archive` to override the defaults.

The rqbit scratch image defines `/home/rqbit/db` and `/home/rqbit/cache` as its
persistence locations and runs `/bin/rqbit` as its entrypoint. It does not
implement LinuxServer-style `PUID` or `PGID` variables. If you add Compose
`user:`, make the two rqbit volumes and the shared `/data` tree writable by that
numeric UID/GID first.

## Sonarr and Radarr settings

Add the built-in **qBittorrent** download client in each application:

| Setting | Sonarr | Radarr |
| --- | --- | --- |
| Host | `127.0.0.1` | `127.0.0.1` |
| Port | `3030` | `3030` |
| Use SSL | off | off |
| Username/password | `RQBIT_USER` / `RQBIT_PASSWORD` | same |
| Category | `sonarr` | `radarr` |
| Initial state | Start | Start |
| Content layout | Original | Original |
| Sequential order | off | off |
| First and last pieces first | off | off |

Create the `sonarr` and `radarr` categories through each client's **Test** flow.
Categories inherit rqbit's global download directory; they do not move content.
Do not configure a Remote Path Mapping: `/data` has the same meaning in all
three containers.

Enable **Use Hardlinks instead of Copy** in Sonarr/Radarr. The download and
library directories must be on the same host filesystem beneath `DATA_ROOT`.
The import leaves rqbit's download path in place for seeding; later removal with
`deleteFiles=true` unlinks only rqbit's managed path, so the library hardlink
remains.

## Migration and operational notes

1. Stop new grabs in Deluge and let in-progress downloads finish.
2. Back up the Deluge and ARR configuration and the rqbit persistence volumes.
3. Start rqbit, then run **Test** for Sonarr and Radarr and perform one legal
   manual grab in each category.
4. Confirm the reported content path is below `/data/downloads/torrents` and
   that an imported library file has the same device and inode as its download
   on Linux (`stat -c '%d:%i %n' <download> <library>`).
5. Test both ARR removal policies before retiring Deluge.

The compatibility profile reports qBittorrent Web API `2.8.1`. Per-torrent
ratio and active-complete seeding-time limits are durable and stop a completed
torrent by pausing it. Inactive-seeding-time limits, sequential downloading,
first/last-piece priority, and queue reordering are intentionally unsupported;
requests for those behaviors fail explicitly.

Unresolved magnets are visible immediately as `metaDL`, but unresolved pending
entries are currently in-memory only. A daemon restart during metadata lookup
requires the sender to submit the magnet again.

NordVPN's ordinary servers do not provide inbound port forwarding. rqbit still
uses trackers, DHT, and outbound peer connections through Gluetun, but peer
reachability and seeding performance can be lower. Dynamic VPN listen-port
integration is outside this compatibility feature.
