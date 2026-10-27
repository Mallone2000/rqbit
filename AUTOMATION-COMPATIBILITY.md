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
| Client configuration | `app/preferences` | Real download directory and DHT configuration; queueing and global share limits disabled |
| Add torrents | `torrents/add` | Multipart torrent files or magnets; URL-encoded magnets; case-insensitive boolean forms |
| Initial state | `paused`, `torrents/setForceStart` | Start, pause, or force start; forced mode bypasses share limits and is cleared by Pause |
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

Queueing remains disabled. `topPrio` deliberately returns 409, matching
qBittorrent with queueing disabled; both reviewed Arr clients handle that status.
Thus Arr's queue-position preference has no scheduling effect. There is no
concurrency-limited torrent queue in this fork.

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

Regression coverage includes realistic form/multipart requests, unauthorized
requests, read-only mode, bad limits, pending-state persistence, managed-state
restoration, write-failure rollback, force-start behavior, content paths,
selection-aware piece order, tracker health, hardlinks, and traversal rejection.
PostgreSQL record conversion is tested without a database; live PostgreSQL restart
tests require a dedicated `RQBIT_TEST_POSTGRES_URL` and are ignored by default.
These tests are not an end-to-end certification with running Sonarr, Radarr, and
Cleanuparr containers, nor a complete security audit.
