# Security review — 2026-10-27

This review hardens the HTTP authentication boundary and checks the resolved Rust
and JavaScript dependencies. It is not a proof that the entire BitTorrent client,
operating system, reverse proxy, or deployment is free of vulnerabilities.

## Changes and compatibility

- Every HTTP listener requires configured username/password credentials, including
  loopback and read-only listeners. Optional Bearer credentials remain scoped to
  `/api/v2`; they do not grant access to native APIs. Missing credentials fail closed.
- UPnP HTTP routes now share native authentication. Anonymous DLNA players no
  longer have access. Login HTML/assets and authentication discovery/login routes
  remain public; they expose no torrent data.
- Bearer keys require 32–256 token characters with at least 32 before padding.
  This rejects common configuration mistakes, but does not measure actual entropy:
  generate a random key, rather than repeating characters or using a sentence.
- Password and token hashes use `subtle` constant-time equality, replacing a
  hand-written comparison whose machine-code timing was not guaranteed. This
  small dependency is already present transitively in the lockfile.
- Sessions use random 256-bit credentials, store only SHA-256 hashes, enforce
  server-side expiration, reject duplicate SID cookies, and revoke on logout.
  Session validation scans the bounded session store without secret-dependent
  comparison exits. Authentication failure tracking no longer evicts active bans
  under client-table pressure; new untracked clients are denied while full. Login
  requests recheck bans after reading their body, closing an in-flight form race.
- Cookies default to Secure/HttpOnly/SameSite=Strict. Plaintext cookies require an
  explicit insecure-cookie option. Proxy headers cannot weaken this setting.
- Browser origin protection covers reads and UPnP as well as mutations. Duplicate
  source headers, non-web schemes, and cross-site Fetch Metadata are rejected.
- Sensitive responses use no-store, nosniff, anti-framing and referrer controls.
- Torrent HTML/SVG/script/PDF downloads cannot execute on the management origin:
  non-media downloads are attachments with an octet-stream MIME type, and all
  streamed torrent content receives a sandbox CSP. Media range streaming remains.

## Dependency audit

The RustSec scan initially reported 13 vulnerability entries plus maintenance and
soundness warnings. Compatible updates address ctutils, quinn-proto, rustls,
rustls-webpki, webpki-root-certs, and Tokio. The shared npm lockfile updates Vite,
Vitest/mocker and source-map-js; its post-update audit reports zero vulnerabilities.

Two Rust findings remain in upstream dependency chains and are not suppressed:

- [RUSTSEC-2026-0293](https://rustsec.org/advisories/RUSTSEC-2026-0293.html):
  librqbit-utp 0.7.0 requires ringbuf 0.4.8; its compatible releases do not yet use
  fixed ringbuf >=0.5.2. The reported double-drop requires an element destructor
  to panic. This caller uses `SharedRb<Heap<u8>>`, whose elements have no destructor;
  that specific trigger is not reachable through these bytes. The dependency
  remains flagged and should be upgraded when its upstream requirement changes.
- [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html):
  the Linux Tauri/GTK desktop dependency chain retains glib 0.18.5, whose
  VariantStrIter implementation is unsound. Fixed glib >=0.20 is incompatible with
  the current GTK dependency chain. This dependency is not used by the server-only
  Docker build. Linux desktop code requires a separate dependency migration and
  Linux verification; this review does not claim it is fixed.

RustSec also reports unmaintained proc-macro-error and unic-* crates in the desktop
chain. These maintenance notices are not evidence of an authentication bypass.
No audit ignore entries were added. The full workspace RustSec audit therefore
still reports the ringbuf vulnerability and desktop warnings.

## Deployment boundary

Run the backend behind HTTPS on a private interface/network. Basic and Bearer
credentials do not encrypt HTTP. Use strong, unique passwords and random tokens;
restart to clear sessions after credential changes and rotate exposed tokens.
The reverse proxy must preserve Host and strip untrusted forwarded headers. Login
throttling uses the socket peer address; behind a proxy, clients may share a limit.
Apply additional proxy rate limits where needed. Public BitTorrent peer/tracker
ports intentionally implement the protocol without management credentials; HTTP
login requirements do not turn torrent data exchange into private file sharing.

Local users who can modify download directories, process memory, environment,
configuration, or the running binary are outside this HTTP authentication boundary.
Static symlink validation does not provide isolation from concurrent filesystem
changes by such users. Run rqbit as a dedicated unprivileged user with restricted
volumes. Do not grant create-torrent access to untrusted authenticated clients.

Session recommendations were checked against the [OWASP Session Management Cheat
Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html).
Constant-time comparisons use the [subtle crate](https://docs.rs/subtle/latest/subtle/trait.ConstantTimeEq.html).


## Validation performed

- `cargo fmt --all -- --check` and `git diff --check` passed.
- `cargo check` and `cargo clippy --all-targets` passed. They retain an existing
  deprecated `fetch_update` warning in JSON persistence.
- `cargo test --workspace --exclude rqbit-desktop`: 286 passed, 9 ignored.
  Includes real HTTP auth/token/UPnP protection checks, HTML streaming and ranges,
  session expiry/revocation, cookie defaults/opt-in, and in-flight login bans.
- `npm test --workspace crates/librqbit/webui`: 48 passed.
- Web UI and desktop frontend production builds passed.
- Native `cargo build -p rqbit-desktop` passed on macOS.
- Compose configuration validation passed for the edited server example.
- npm audit reports zero vulnerabilities. RustSec retains the findings listed
  above; no clean full-workspace RustSec audit is claimed.

Tests ran on macOS. Linux desktop execution, adversarial protocol fuzzing, a fresh
Docker image build, and deployed reverse-proxy/TLS configuration were not verified.
No changes were pushed or deployed.

## Follow-up review — 2026-11-05

- Successful native Web UI and qBittorrent logins revoke the SID presented with
  the login request before issuing a replacement. Failed login leaves existing
  sessions intact; independent clients retain their sessions.
- Successful background Basic/Bearer API traffic no longer resets the address's
  password failure counter. This prevents automation behind a shared proxy from
  repeatedly clearing an attacker's failed guesses. Successful form login still
  clears earlier failures.
- Sessions have a 15-minute idle timeout in addition to the existing one-hour
  absolute lifetime. Authenticated requests refresh activity without extending
  the absolute deadline. Logout cookie deletion preserves Secure by default.
- Fetch Metadata from sibling origins cannot bypass source checks by suppressing
  Origin/Referer. Duplicate Fetch Metadata headers fail closed. Non-browser
  automation without browser source headers remains supported.
- The default content policy restricts scripts, connections, and forms to the
  management origin, while retaining inline styles used by the UI. The stricter
  sandbox policy on untrusted torrent streams is preserved.
- Torrent validation rejects zero piece length, overflowing file-length sums,
  unrepresentable piece/chunk counts, and inconsistent piece-hash tables before
  allocating torrent tracking state. These inputs could otherwise panic (aborting
  the process in release builds), wrap counters, or inflate allocations.
- Torrent creation no longer appends an empty piece hash at exact boundaries and
  rejects zero piece length. Validation retains the narrowly identified legacy
  empty trailing hash so existing sessions created by rqbit still restore.
- Refreshed npm audit reports zero vulnerabilities. Refreshed RustSec still
  reports ringbuf 0.4.8 (RUSTSEC-2026-0293), glib 0.18.5 unsoundness, and the
  maintenance warnings described above. `cargo info librqbit-utp` still resolves
  to 0.7.0. These findings are not suppressed or claimed fixed. Resolving them
  requires upstream changes or separately maintained dependency backports; a
  forced semver-incompatible transitive upgrade would not be a verified fix.

This follow-up does not certify that every security issue has been found. HTTPS,
reverse-proxy configuration, host access controls, and Linux desktop behavior
must still be verified in the actual deployment. No deployment was performed.

### Follow-up validation

- `cargo fmt --all -- --check` and `git diff --check` passed.
- `cargo check` and `cargo clippy --all-targets` passed, retaining the existing
  JSON persistence `fetch_update` deprecation warning.
- `cargo test --workspace --exclude rqbit-desktop -- --test-threads=1` passed:
  303 tests passed, 11 ignored. This includes HTTP authentication/compatibility,
  session rotation/idle and absolute expiry, throttling amid successful API
  traffic, browser source checks, torrent validation/creation, persistence,
  streaming and TCP/uTP transfer tests.
- The oversized metadata HTTP test server now reads the request before replying;
  its size-limit assertion is unchanged. UPnP test torrents now include the
  declared piece hashes, so they exercise valid metadata under stricter validation.
- npm audit reports zero vulnerabilities. RustSec findings remain as listed above.
- Linux desktop builds, frontend browser execution, a new Docker image, and the
  deployed TLS/reverse-proxy configuration were not verified in this follow-up.
  Nothing was pushed or deployed.

## Additional boundary review — 2026-11-05

- Web UI and qBittorrent login form reads now have a 10-second deadline in
  addition to the existing 8 KiB limit. An unfinished body returns HTTP 408 and
  cannot issue a session; HTTP/1 connections are closed rather than draining the
  unfinished body for reuse. This limits how long unauthenticated slow bodies retain
  login-handler resources; it is not a global connection/concurrency limit.
  Authenticated uploads and long-lived streaming keep their existing behavior.
- A peer could send a validly encoded `ut_metadata` data message before the
  extended handshake and reach an `unwrap()` on absent transfer state. A second
  data message after completion could reach the same assumption. Release builds
  abort on panic, making this a remotely triggerable denial of service during
  magnet metadata discovery. Both cases now return connection errors.
- Repeated metadata handshakes retain received pieces and do not allocate a new
  buffer or enqueue another full set of requests. Peers that change metadata size
  during transfer are disconnected, as are handshakes after completion. Zero-size
  metadata is rejected before initialization. Normal initial handshakes and
  verified metadata completion remain supported.
- Refreshed npm audit reports zero vulnerabilities. Refreshed RustSec still
  reports ringbuf 0.4.8, desktop glib unsoundness, and the maintenance notices
  documented above. `cargo info librqbit-utp` resolves to 0.7.0. No dependency
  findings were suppressed; these findings remain unresolved. Fixing those chains
  needs a maintained backport or dependency migration with appropriate platform
  testing, rather than an unchecked incompatible version override.
- The existing HTTP security tests cover valid/invalid/missing credentials,
  Basic/Bearer scope, session rotation, revocation and expiry, cookie security,
  throttling and browser-origin checks. This review did not establish an
  authentication bypass. Passing those checks is not proof that authentication
  is unbreachable.

The workspace version advances to `9.0.1-servarr.14`. No changes were pushed or
deployed, and production HTTPS/proxy settings and Linux desktop behavior remain
outside the validation performed here.

### Additional review validation

- `CARGO_INCREMENTAL=0 cargo test --workspace --exclude rqbit-desktop --offline -- --test-threads=1`
  passed: 308 tests passed, none failed, 11 ignored. This includes
  HTTP compatibility/authentication, persistence and TCP/uTP transfer tests.
- `cargo fmt --all -- --check`, `git diff --check`, `cargo check --offline`, and
  `CARGO_INCREMENTAL=0 cargo clippy --all-targets --offline` passed. Clippy retains
  only the existing JSON persistence `fetch_update` deprecation warning.
- Final targeted HTTP authentication tests passed (11), including real socket
  checks that both slow login endpoints return 408, close the connection, issue
  no cookie, and still accept subsequent valid logins. Final peer metadata
  regression tests passed (4), covering unsolicited/replayed data, zero/oversized
  metadata, repeated handshakes and successful completion.
- `npm test --workspace crates/librqbit/webui` passed all 48 tests. The refreshed
  `npm audit` reports zero vulnerabilities; `cargo audit` still exits unsuccessfully
  for the unresolved findings above.
- An initial reduced-feature Rust test command exposed the pre-existing requirement
  for `tracing-subscriber-utils` in HTTP handlers/tests; the corrected feature set
  compiled and passed. Broader attempts initially ran out of disk space and lacked
  permission to bind test sockets. Generated project build artifacts were cleared
  and tests were rerun with local socket access.

## Login admission review — 2026-11-06

- Native Web UI and qBittorrent login endpoints share a maximum of 16 pending
  form-body reads per HTTP server. Admission is immediate: excess callers receive
  HTTP 429 and an HTTP/1 connection-close response, without waiting for or draining
  the unfinished body. Permits release on completion, malformed bodies, timeout,
  and disconnect. The existing 8 KiB limit and 10-second deadline remain in place.
  This bounds login-body processing, not all HTTP connections or transport buffers;
  deployment connection limits remain the HTTPS proxy's responsibility.
- A late authentication failure no longer resets an existing ban when the original
  failure window has elapsed. This closes a concurrency edge case for callers that
  passed admission before another request recorded the ban.
- Browser source validation rejects unrecognized Fetch Metadata values even when
  Origin or Referer is present, and rejects Origin values containing non-root paths, query
  strings, or user information. Normal Referer paths remain supported.
- Existing session entropy, hashed server-side tokens, rotation, logout revocation,
  absolute/idle expiration, secure cookie defaults, Basic/Bearer scope and throttling
  remain covered by the authentication regression suite.
- Refreshed dependency audits: npm reports zero vulnerabilities. RustSec still
  reports ringbuf 0.4.8 (RUSTSEC-2026-0293), desktop glib unsoundness and the
  maintenance warnings described above. The published librqbit-utp version remains
  0.7.0; no compatible fixed dependency release was found. These findings have not
  been suppressed or represented as fixed.

The workspace version advances to `9.0.1-servarr.16`. This review does not guarantee
unbreachable authentication or establish that all project security issues are fixed.
Production TLS/proxy settings, platform-specific desktop behavior, and adversarial
protocol fuzzing still require separate verification. No changes were pushed or deployed.

### Login admission validation

- `cargo fmt --all -- --check`, `git diff --check`, `cargo check --offline`, and
  `CARGO_INCREMENTAL=0 cargo clippy --all-targets --offline` passed. The existing
  JSON persistence `fetch_update` deprecation warning remains.
- `CARGO_INCREMENTAL=0 cargo test -p librqbit --features
  http-api,webui,tracing-subscriber-utils http_api:: --offline -- --test-threads=1`
  passed all 55 tests, including shared login-body capacity, immediate connection
  closure without session issuance, recovery after disconnect, late-failure ban
  retention, malformed browser source checks, and existing authentication/session
  and compatibility tests. The initial restricted run could not bind sockets;
  these tests passed when rerun with local socket access.
- `npm audit --json` reports zero vulnerabilities. `cargo audit --json` still exits
  unsuccessfully for the unresolved RustSec finding documented above; glib and
  maintenance warnings also remain. No dependency ignore entries were added.
- `CARGO_INCREMENTAL=0 cargo test --workspace --exclude rqbit-desktop --offline
  -- --test-threads=1` passed: 311 tests passed, 0 failed, 11 ignored. This covers
  the server workspace, including authentication/compatibility, session persistence,
  filesystem safety and TCP/uTP transfers. Native desktop builds, frontend browser
  execution, Docker publication and the deployed TLS/proxy configuration were not
  verified in this review.
