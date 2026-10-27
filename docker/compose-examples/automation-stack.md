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
For Cleanuparr's optional API key connection, add a random 32–256 character
`RQBIT_QBITTORRENT_API_KEY` secret to rqbit's environment and configure the
same key in Cleanuparr. Keep the username/password setting for rqbit's native
API and Sonarr/Radarr clients. Browser cookies require HTTPS by default. For an
explicitly trusted plaintext network only, set `RQBIT_HTTP_API_INSECURE_COOKIES=true`;
this does not encrypt HTTP credentials or cookies. UPnP HTTP endpoints also require
authentication; anonymous DLNA players are unsupported.

The Compose example uses `ghcr.io/mallone2000/rqbit:latest`. A successful build
of the `main` branch publishes that rolling multi-architecture image. Update an
existing deployment without building or transferring an archive:

```sh
docker compose pull rqbit
docker compose up -d rqbit
```

After the first workflow publish, set the GHCR package visibility to **Public**
once so the deployment can pull it without registry credentials.

The `latest` tag intentionally follows `main`. Pin the image by digest instead
when deployments must not change until explicitly updated.

For local development or an unpublished revision, Windows users can still build
and export a Linux image from the repository root:

```powershell
.\scripts\build-docker-image.ps1
```

The local build's default output is `rqbit:test` plus a timestamped
`target/rqbit-linux-amd64-*.tar` archive. Use `-Platform linux/arm64` for an
ARM64 server, or `-Image` and `-Archive` to override the defaults.

The rqbit scratch image defines `/home/rqbit/db` and `/home/rqbit/cache` as its
persistence locations and runs `/bin/rqbit` as its entrypoint. It does not
implement LinuxServer-style `PUID` or `PGID` variables. If you add Compose
`user:`, make the two rqbit volumes and the shared `/data` tree writable by that
numeric UID/GID first.

## Sonarr and Radarr settings

Add the built-in **qBittorrent** download client in each application:

| Setting                     | Sonarr                          | Radarr      |
| --------------------------- | ------------------------------- | ----------- |
| Host                        | `127.0.0.1`                     | `127.0.0.1` |
| Port                        | `3030`                          | `3030`      |
| Use SSL                     | off                             | off         |
| Username/password           | `RQBIT_USER` / `RQBIT_PASSWORD` | same        |
| Category                    | `sonarr`                        | `radarr`    |
| Initial state               | Start                           | Start       |
| Content layout              | Original                        | Original    |
| Sequential order            | off                             | off         |
| First and last pieces first | off                             | off         |

Create the `sonarr` and `radarr` categories through each client's **Test** flow.
Categories inherit rqbit's global download directory; they do not move content.
Do not configure a Remote Path Mapping: `/data` has the same meaning in all
three containers.

rqbit stops unresolved magnets after five minutes and exposes them as `error`
through its qBittorrent API. It does not contact Sonarr or Radarr. If you use
Cleanuparr, connect it to rqbit and both Arr applications, configure its
[Queue Cleaner](https://cleanuparr.github.io/docs/configuration/queue-cleaner/)
for metadata and error states, and enable [Seeker's replacement search](https://cleanuparr.github.io/docs/configuration/seeker/).
Let Cleanuparr remove the failed item from the Arr queue so it can request a
replacement; deleting it only in rqbit cannot do that.

Cleanuparr's Malware Blocker, Download Cleaner, tagging, and blacklist
synchronizer use the same qBittorrent API. Give Cleanuparr access to the same
`/data` download tree to enable unlinked and orphaned file checks. Its
blacklist sync updates the excluded-name pattern list; explicitly enable
`excluded_file_names_enabled` through the qBittorrent preferences API before
expecting new additions to skip those names. Existing torrents keep their
saved file selections. Tracker seeder counts remain `-1` until a tracker
reports them, and trackerless torrents may remain unknown.

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

Unresolved magnets are visible immediately as `metaDL`; paused metadata jobs
report `pausedDL`. Pending submissions, pause state, settings, and retry
diagnostics survive daemon restarts when JSON/file or PostgreSQL session
persistence is enabled. The mounted state directory/database must remain
available. The Web UI and hash-based API routes can pause, resume/retry, or
remove pending jobs while keeping existing files.

Back up rqbit state before upgrading. The new pending collection/table is
additive, but do not share writable state between different versions. Protect
state and backups because saved magnets can contain private tracker credentials.
Before downgrading, preserve pending submissions: an older JSON writer discards
the new collection; an older PostgreSQL build retains the table but cannot
manage those jobs. See the [pending persistence and rollback notes](../../README.md#qbittorrent-web-api-compatibility).

NordVPN's ordinary servers do not provide inbound port forwarding. rqbit still
uses trackers, DHT, and outbound peer connections through Gluetun, but peer
reachability and seeding performance can be lower. Dynamic VPN listen-port
integration is outside this compatibility feature.
