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
