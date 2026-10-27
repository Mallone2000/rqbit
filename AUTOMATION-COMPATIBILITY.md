# Sonarr, Radarr, and Cleanuparr compatibility

This fork implements the qBittorrent download-client contract used by these
applications. Their own REST APIs control library management, searches, queues,
and blocklisting; those features remain in the applications. rqbit does not
receive Arr API keys or initiate replacement searches.

## Evidence reviewed

Reviewed on 2026-10-27 against these source revisions:

- [Sonarr adapter and proxy](https://github.com/Sonarr/Sonarr/tree/a6171f0ca61b2a76ec529da3e9aadb11435344f9/src/NzbDrone.Core/Download/Clients/QBittorrent).
- [Radarr adapter and proxy](https://github.com/Radarr/Radarr/tree/e77fd33e82d1d80f1cc58c7a5ebd959a90210299/src/NzbDrone.Core/Download/Clients/QBittorrent).
- [Cleanuparr qBittorrent services](https://github.com/Cleanuparr/Cleanuparr/tree/51b8776776e5a6d7b748bd03eec48bed80698d29/code/backend/Cleanuparr.Infrastructure/Features/DownloadClient/QBittorrent).
- [qBittorrent HTTP implementation](https://github.com/qbittorrent/qBittorrent/blob/f6cf56c5fd85ce32c1af54566d43bb476dbcc4cd/src/webui/api/torrentscontroller.cpp)
  and [torrent implementation](https://github.com/qbittorrent/qBittorrent/blob/f6cf56c5fd85ce32c1af54566d43bb476dbcc4cd/src/base/bittorrent/torrentimpl.cpp).
- User-facing [Sonarr API documentation](https://sonarr.tv/docs/api/#v5) and
  [Radarr API documentation](https://radarr.video/docs/api/). The download-client
  requirements below come from the adapters, rather than the Arr REST schemas.

## Consumer contract

All routes below are under `/api/v2`. Authentication and browser-origin checks
apply to reads as well as writes. Mutations are absent in read-only mode.

| Workflow | Routes / fields | Behavior |
| --- | --- | --- |
| Connection and authentication | `auth/login`, `auth/logout`, `app/webapiVersion`, `app/version` | Cookie login/logout, Basic credentials, optional Bearer key scoped to this API; advertised API version 2.9.2 |
| Client configuration | `app/preferences` | Real download directory and DHT configuration; real persisted queue settings; global share limits disabled |
| Add torrents | `torrents/add` | Multipart torrent files or magnets; URL-encoded magnets; case-insensitive boolean forms |
| Initial state | `paused`, `torrents/setForceStart` | Start, pause, or force start; forced mode bypasses queue and share limits and is cleared by Pause |
| Torrent queue | preferences, `topPrio`, `bottomPrio`, `increasePrio`, `decreasePrio` | Opt-in download/seed/total limits; durable order; seed ranking; force-start bypass; slow-torrent accounting; queued states |
| Download order | `sequentialDownload`, `firstLastPiecePrio` | Sequential piece order; prioritize the first and last 1% of every wanted file using qBittorrent's edge-size calculation; preserve file exclusions |
| Content layout | `contentLayout=Original` / `Subfolder` | Original rqbit layout, or require a folder for single-file torrents too; validate folder paths |
| Share limits | `ratioLimit`, `seedingTimeLimit`, `inactiveSeedingTimeLimit`, `torrents/setShareLimits` | Per-torrent limits, minutes converted to seconds; apply to pending and managed torrents; omitted fields preserve existing settings |
| Polling / importing | `torrents/info`, `torrents/properties`, `torrents/files` | Category/hash selection, completed filter, pending visibility, progress, completion, activity, ratios, paths, file selection |
| Categories and post-import tags | `torrents/categories`, `createCategory`, `removeCategories`, `setCategory`, `addTags` | Durable metadata; category assignment does not move files |
| Pause, retry, delete | `torrents/pause`, `resume`, `stop`, `start`, `delete` | Pending and managed torrents; explicit data-deletion flag; authenticated start/stop aliases |
| Tracker and privacy checks | `torrents/trackers`, `properties.is_private`, `info.num_complete` | Real announce results and private flag; unknown seed counts remain -1 |
| Malware Blocker | `torrents/files`, `torrents/filePrio` | File names, sizes, progress and skip selection; skipped files remain excluded from edge prioritization |
| Blacklist sync | `app/setPreferences` | Persist excluded-file wildcard patterns; filtering must be enabled explicitly |
| Download Cleaner | paths, tags, category, ratio, seeding time, activity | `save_path` joined with a file's `name` addresses the actual download file, including multi-file folders |
| Alternate-speed probe | `transfer/speedLimitsMode` | Disabled; rqbit has no alternative-speed schedule |

## Limits and lifecycle

Seeding duration counts completed, live time. Inactive seeding limits use elapsed
wall-clock time since observed payload activity, including downtime. Existing
activity timestamps survive restoration; file verification is not new payload
activity. A completed torrent with no recorded activity starts its baseline when
first observed. Reaching any configured share limit pauses the torrent, except
in forced mode. The global limits reported by preferences are disabled.

Settings are additive fields in shared automation metadata, persisted by both
JSON and PostgreSQL storage. Old records default to disabled settings. Pending
magnets carry these settings through metadata resolution. Persistence failures
must not report a successful settings change. Malformed PostgreSQL automation
metadata now causes a restoration error instead of silently resetting settings.

Queueing is disabled by default and configurable through the native API,
qBittorrent preferences, CLI/environment and web interface. Defaults when enabled
are 3 downloads, 3 seeds and 5 total. JSON/PostgreSQL store settings and ordered
hashes atomically. Combined queue/exclusion preference writes use one atomic file
update or SQL transaction. Automatic holds never become manual pauses. Startup
configuration loads before restoration; explicit overrides apply after it.

The algorithm was checked against the qBittorrent revision above and
[libtorrent 2.0.11 admission control](https://github.com/arvidn/libtorrent/blob/v2.0.11/src/session_impl.cpp)
and [seed ranking / inactive transitions](https://github.com/arvidn/libtorrent/blob/v2.0.11/src/torrent.cpp).
Downloads take precedence. Seeds use share-target flags, swarm scarcity and
startup protection with stable hash tie-breaking. Unknown scrape counts fall
back to peer observations, retained while held. Slow-state changes use the
corresponding direction’s threshold and a monotonic debounce timer. Only the
separate download/seed limits exempt slow torrents; the total remains binding.
Forced torrents consume no automatic slots. rqbit’s peer discovery and transfer
rate estimation remain engine-specific.

Priority routes return 409 while disabled and perform stable moves otherwise,
including `hashes=all`. Priorities are one-based for downloads and -1 for seeds
or disabled queueing under the advertised 2.9.2 profile. Pending magnets remain
visible as queued before discovery. Queue waiting does not spend their timeout
budget. Displacement cancels discovery before replacements are admitted;
pause/deletion cancel the whole job. See README for settings and native routes.

HTTP torrent-URL fetching is deliberately unavailable through the compatibility
API. The reviewed Arr adapters fetch torrent files themselves and submit bytes;
their URL submission path submits magnets. This restriction avoids adding a
server-side arbitrary HTTP fetch surface. Flattened `NoSubfolder` layout is not
supported; the reviewed Arr settings use Original or Subfolder. Pending magnet
submission requires a BitTorrent v1 info hash; v2-only magnets are not accepted.
This is not a complete qBittorrent API implementation.

Cleanuparr's unlinked/orphan cleanup needs filesystem access and matching path
mapping; its Seeker and blocklist workflows need its Arr connections. Unknown
tracker seed counts cannot establish that a torrent is dead. rqbit does not
implement Cleanuparr's rules itself.

## Security and validation

Enable the compatibility API explicitly and configure nonempty credentials.
Use TLS at a trusted reverse proxy and restrict access to the automation network.
Credentials, SID cookies, complete magnets, and tracker passkeys must not appear
in logs. Compatibility error logs contain operation names, without arbitrary
backend error chains. Malformed browser-origin headers fail closed. Existing
login throttling, same-origin checks, bounded uploads, and symlink protections
remain in effect.

Queue regressions cover admission limits, stable reordering, forced mode, slow-state
debouncing, concurrent preference patches, queued pending visibility, manual and
share-limit stops, restart ordering and failed-write rollback.

Regression coverage includes realistic form/multipart requests, unauthorized
requests, read-only mode, bad limits, pending-state persistence, managed-state
restoration, write-failure rollback, force-start behavior, content paths,
selection-aware piece order, tracker health, hardlinks, and traversal rejection.
PostgreSQL record conversion is tested without a database; live PostgreSQL restart
tests require a dedicated `RQBIT_TEST_POSTGRES_URL` and are ignored by default.
These tests are not an end-to-end certification with running Sonarr, Radarr, and
Cleanuparr containers, nor a complete security audit.
