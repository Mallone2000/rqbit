# Production HTTPS

The base `server.yaml` publishes rqbit's plaintext management port only on
`127.0.0.1:3030`. It uses bridge networking, disables UPnP discovery and automatic
router port forwarding, and keeps Secure cookies enabled. TCP and UDP port 4240
remain public for BitTorrent peers. If inbound peer connections are needed,
forward only that peer port manually on your router.

For browser access, add the Caddy TLS overlay. Set a public DNS hostname whose
A/AAAA records point to your server, and allow inbound TCP ports 80 and 443 for
certificate issuance and HTTPS. UDP 443 enables HTTP/3. Keep port 3030 closed on
your router and firewall. The hostname must be a bare DNS name, without a scheme,
path, or port. If a reverse proxy already owns 80/443, configure that proxy instead
of starting a second one.

From the repository root, create a private, untracked environment file (for
example outside the checkout) with `RQBIT_USER`, a strong unique `RQBIT_PASSWORD`,
and `RQBIT_DOMAIN`. Protect it with `chmod 600`. Then run:

```sh
docker compose --env-file /path/to/rqbit.env \
  -f docker/compose-examples/server.yaml \
  -f docker/compose-examples/https.yaml config --quiet
docker compose --env-file /path/to/rqbit.env \
  -f docker/compose-examples/server.yaml \
  -f docker/compose-examples/https.yaml up -d
```

Replace the base example's download mount before starting. Keep the same Compose
project name and state volumes when migrating an existing deployment, and back up
state first. Caddy's persistent `/data` volume retains certificates and account
keys across restarts; protect it and include it in backups.

Open `https://<your-hostname>/web/` and sign in with the configured rqbit
credentials. Caddy obtains and renews certificates and redirects HTTP to HTTPS.
Always configure remote API clients with HTTPS directly: a redirect cannot
protect credentials already sent over HTTP. Do not enable insecure cookies,
disable certificate validation, or expose the backend port on every interface.

The proxy preserves the original Host so rqbit's browser origin checks work. It
removes forwarded headers before passing requests to rqbit; rqbit does not trust
these headers for identity or cookie security. Do not enable broad CORS allowances
or rewrite Origin/Referer to bypass origin checks. Login throttling sees the proxy
address, so failures can temporarily block other clients using that proxy. For
multiple users, apply per-client throttling at your external ingress as well.
The included proxy has no access log, avoiding storage of credential-bearing
request URLs. rqbit still enforces authentication on its own routes.

For an existing proxy on the Docker host, use `127.0.0.1:3030` as its upstream.
A proxy in another container must reach rqbit over a shared private Docker
network using `rqbit:3030`; its own loopback address points to itself. Preserve
the public Host including any nonstandard port and strip untrusted forwarded
headers. Keep TLS verification enabled between remote clients and the proxy.

The Gluetun automation topology is separate: retain the shared namespace and
`127.0.0.1:3030` for Sonarr/Radarr communication described in
[the automation guide](automation-stack.md). Do not combine this bridge-network
example with Gluetun's network namespace unchanged.

After deployment, check that HTTP redirects to HTTPS, the certificate validates,
login sets a Secure/HttpOnly/SameSite cookie, cross-origin requests receive 403,
and the management port is unreachable from another machine. These live checks
require your DNS, firewall, and running server; Compose validation alone cannot
prove them.

References: [Caddy automatic HTTPS](https://caddyserver.com/docs/automatic-https)
and [reverse proxy headers](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy#headers).
