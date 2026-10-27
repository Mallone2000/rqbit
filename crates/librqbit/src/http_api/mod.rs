use anyhow::Context;
use axum::extract::{ConnectInfo, Request};
use axum::middleware::Next;
use axum::response::IntoResponse;
#[cfg(any(feature = "webui", feature = "prometheus"))]
use axum::routing::get;
use base64::Engine;
use futures::FutureExt;
use futures::future::BoxFuture;
use http::{HeaderMap, StatusCode};
use librqbit_dualstack_sockets::TcpListener;
use sha1w::{ISha256, Sha256};
use std::sync::Arc;
use subtle::ConstantTimeEq;
use tower_http::trace::{DefaultOnFailure, DefaultOnResponse, OnFailure};
use tracing::{Span, debug_span, info};

use axum::Router;

use crate::api::Api;

use crate::ApiError;
use crate::api::Result;

mod handlers;
mod qbittorrent;
mod timeout;
#[cfg(feature = "webui")]
mod webui;

/// An HTTP server for the API.
pub struct HttpApi {
    api: Api,
    opts: HttpApiOptions,
    qbittorrent_sessions: qbittorrent::auth::Sessions,
    public_ip_client: reqwest::Client,
    public_ip_lookup_url: String,
}

#[derive(Default)]
pub struct HttpApiOptions {
    pub read_only: bool,
    pub basic_auth: Option<(String, String)>,
    /// Explicit opt-in for session cookies over plaintext HTTP on trusted networks.
    pub insecure_session_cookies: bool,
    /// Optional Bearer credential accepted only by the qBittorrent compatibility API.
    pub qbittorrent_api_key: Option<String>,
    // Allow creating torrents via API.
    pub allow_create: bool,
    /// Maximum upload body size.
    pub max_upload_body_size: Option<usize>,
    /// Expose the qBittorrent Web API compatibility routes.
    pub enable_qbittorrent_api: bool,
    #[cfg(feature = "prometheus")]
    pub prometheus_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
}

impl std::fmt::Debug for HttpApiOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = formatter.debug_struct("HttpApiOptions");
        debug
            .field("read_only", &self.read_only)
            .field("insecure_session_cookies", &self.insecure_session_cookies)
            .field(
                "basic_auth",
                &self.basic_auth.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "qbittorrent_api_key",
                &self.qbittorrent_api_key.as_ref().map(|_| "<redacted>"),
            )
            .field("allow_create", &self.allow_create)
            .field("max_upload_body_size", &self.max_upload_body_size)
            .field("enable_qbittorrent_api", &self.enable_qbittorrent_api);
        #[cfg(feature = "prometheus")]
        debug.field("prometheus_handle", &self.prometheus_handle);
        debug.finish()
    }
}

async fn simple_basic_auth(
    expected_username: Option<&str>,
    expected_password: Option<&str>,
    sessions: &qbittorrent::auth::Sessions,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Result<axum::response::Response> {
    let (expected_user, expected_pass) = match (expected_username, expected_password) {
        (Some(u), Some(p)) => (u, p),
        _ => return Err(ApiError::unauthorized()),
    };
    #[cfg(feature = "webui")]
    if request.uri().path() == "/"
        && headers
            .get(http::header::ACCEPT)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/html"))
    {
        return Ok(next.run(request).await);
    }
    if sessions.authenticates(&headers) {
        return Ok(next.run(request).await);
    }
    let client_ip = request
        .extensions()
        .get::<ConnectInfo<librqbit_dualstack_sockets::WrappedSocketAddr>>()
        .map(|ConnectInfo(address)| address.0.ip());
    if !sessions.login_is_allowed(client_ip) {
        return Ok((
            StatusCode::TOO_MANY_REQUESTS,
            "Too many authentication failures",
        )
            .into_response());
    }
    if !credentials_match_basic(
        Some(&(expected_user.to_owned(), expected_pass.to_owned())),
        &headers,
    ) {
        if headers.contains_key(http::header::AUTHORIZATION)
            || headers.contains_key(http::header::COOKIE)
        {
            sessions.record_login_failure(client_ip);
        }
        if headers.contains_key("x-rqbit-webui") {
            return Ok((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
        }
        if headers.get("Authorization").is_none() {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [("WWW-Authenticate", "Basic realm=\"API\"")],
            )
                .into_response());
        }
        return Err(ApiError::unauthorized());
    }
    sessions.clear_login_failures(client_ip);
    Ok(next.run(request).await)
}

pub(crate) fn credentials_match_basic(
    expected: Option<&(String, String)>,
    headers: &HeaderMap,
) -> bool {
    let Some((expected_user, expected_pass)) = expected else {
        return false;
    };
    let Some((user, pass)) = authorization_credential(headers, "Basic")
        .and_then(|v| base64::engine::general_purpose::STANDARD.decode(v).ok())
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| v.split_once(':').map(|(u, p)| (u.to_owned(), p.to_owned())))
    else {
        return false;
    };
    constant_time_credentials_match(expected_user, expected_pass, &user, &pass)
}

pub(crate) fn constant_time_credentials_match(
    expected_user: &str,
    expected_pass: &str,
    user: &str,
    pass: &str,
) -> bool {
    constant_time_eq(&credential_digest(expected_user), &credential_digest(user))
        & constant_time_eq(&credential_digest(expected_pass), &credential_digest(pass))
}

fn credential_digest(value: &str) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(value.as_bytes());
    digest.finish()
}

pub(crate) fn constant_time_secret_match(expected: &str, supplied: &str) -> bool {
    constant_time_eq(&credential_digest(expected), &credential_digest(supplied))
}

fn constant_time_eq(expected: &[u8; 32], actual: &[u8; 32]) -> bool {
    bool::from(expected.ct_eq(actual))
}

// Reject ambiguous headers and never accept credentials from URLs or query strings.
fn authorization_credential<'a>(headers: &'a HeaderMap, scheme: &str) -> Option<&'a str> {
    let mut values = headers.get_all(http::header::AUTHORIZATION).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    let (actual_scheme, credential) = value.split_once(' ')?;
    (actual_scheme.eq_ignore_ascii_case(scheme) && !credential.is_empty()).then_some(credential)
}

fn valid_api_key(key: &str) -> bool {
    // RFC 6750 b64token syntax, with a minimum length to reject accidental weak keys.
    (32..=256).contains(&key.len())
        && key.trim_end_matches('=').len() >= 32
        && !key.starts_with('=')
        && key
            .trim_end_matches('=')
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._~+/".contains(&byte))
}

fn credentials_match_bearer(expected: Option<&str>, headers: &HeaderMap) -> bool {
    expected
        .zip(authorization_credential(headers, "Bearer"))
        .is_some_and(|(expected, supplied)| {
            valid_api_key(expected)
                && valid_api_key(supplied)
                && constant_time_secret_match(expected, supplied)
        })
}

fn compile_cors_allow_regex(value: &str) -> std::result::Result<regex::bytes::Regex, regex::Error> {
    // An Origin is one complete value. Implicitly anchor custom expressions so
    // a rule for `trusted.example` cannot also authorize
    // `trusted.example.attacker.invalid`.
    regex::bytes::Regex::new(&format!(r"\A(?:{value})\z"))
}

async fn security_headers(request: Request, next: Next) -> axum::response::Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        http::header::CACHE_CONTROL,
        http::HeaderValue::from_static("no-store"),
    );
    headers.insert(
        "x-content-type-options",
        http::HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", http::HeaderValue::from_static("DENY"));
    headers
        .entry("content-security-policy")
        .or_insert(http::HeaderValue::from_static(
            "frame-ancestors 'none'; object-src 'none'; base-uri 'self'",
        ));
    headers.insert(
        "referrer-policy",
        http::HeaderValue::from_static("no-referrer"),
    );
    response
}

impl HttpApi {
    pub fn new(api: Api, opts: Option<HttpApiOptions>) -> Self {
        Self {
            api,
            opts: opts.unwrap_or_default(),
            qbittorrent_sessions: Default::default(),
            public_ip_client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("building the public IP HTTP client should not fail"),
            public_ip_lookup_url: "https://api64.ipify.org".to_owned(),
        }
    }

    /// Run the HTTP server forever on the given address.
    /// If read_only is passed, no state-modifying methods will be exposed.
    #[inline(never)]
    pub fn make_http_api_and_run(
        #[allow(unused_mut)] mut self,
        listener: TcpListener,
        upnp_router: Option<Router>,
    ) -> BoxFuture<'static, anyhow::Result<()>> {
        let credentials_configured = self
            .opts
            .basic_auth
            .as_ref()
            .is_some_and(|(user, pass)| !user.trim().is_empty() && !pass.trim().is_empty());
        if !credentials_configured {
            return async {
                anyhow::bail!("HTTP API requires a non-empty username and password, including loopback and read-only listeners")
            }
            .boxed();
        }
        if self
            .opts
            .qbittorrent_api_key
            .as_deref()
            .is_some_and(|key| !valid_api_key(key))
        {
            return async {
                anyhow::bail!("qBittorrent API key must contain 32 to 256 ASCII token characters; generate a random secret")
            }.boxed();
        }

        #[cfg(feature = "prometheus")]
        let mut prometheus_handle = self.opts.prometheus_handle.take();

        let state = Arc::new(self);

        let mut main_router = handlers::make_api_router(state.clone());

        #[cfg(feature = "prometheus")]
        if let Some(handle) = prometheus_handle.take() {
            let session = state.api.session().clone();
            main_router = main_router.route(
                "/metrics",
                get(move || async move {
                    let mut metrics = handle.render();
                    session.stats_snapshot().as_prometheus(&mut metrics);
                    metrics
                }),
            );
        }

        let cors_layer = {
            use tower_http::cors::{AllowHeaders, AllowOrigin};

            const ALLOWED_ORIGINS: [&[u8]; 4] = [
                // Webui-dev
                b"http://localhost:3031",
                b"http://127.0.0.1:3031",
                // Tauri dev
                b"http://localhost:1420",
                // Tauri prod
                b"tauri://localhost",
            ];

            let allow_regex = std::env::var("CORS_ALLOW_REGEXP")
                .ok()
                .and_then(|value| compile_cors_allow_regex(&value).ok());

            tower_http::cors::CorsLayer::default()
                .allow_origin(AllowOrigin::predicate(move |v, _| {
                    (cfg!(debug_assertions) && ALLOWED_ORIGINS.contains(&v.as_bytes()))
                        || allow_regex
                            .as_ref()
                            .map(move |r| r.is_match(v.as_bytes()))
                            .unwrap_or(false)
                }))
                .allow_headers(AllowHeaders::mirror_request())
                .allow_credentials(true)
        };

        if let Some(upnp_router) = upnp_router {
            main_router = main_router.nest("/upnp", upnp_router);
        }

        // Simple one-user basic auth
        if let Some((user, pass)) = state.opts.basic_auth.clone() {
            info!("Enabling simple basic authentication in HTTP API");
            let auth_state = state.clone();
            main_router = main_router.route_layer(axum::middleware::from_fn(
                move |headers, request, next| {
                    let user = user.clone();
                    let pass = pass.clone();
                    let auth_state = auth_state.clone();
                    async move {
                        simple_basic_auth(
                            Some(&user),
                            Some(&pass),
                            &auth_state.qbittorrent_sessions,
                            headers,
                            request,
                            next,
                        )
                        .await
                    }
                },
            ));
        }

        #[cfg(feature = "webui")]
        {
            use axum::response::Redirect;
            use axum::{middleware, routing::post};

            let webui_router = webui::make_webui_router::<Arc<HttpApi>>()
                .route("/auth/status", get(qbittorrent::auth::web_status))
                .route("/public-ip", get(qbittorrent::auth::web_public_ip))
                .route(
                    "/auth/login",
                    post(qbittorrent::auth::web_login)
                        .layer(axum::extract::DefaultBodyLimit::max(8 * 1024)),
                )
                .route("/auth/logout", post(qbittorrent::auth::web_logout))
                .route_layer(middleware::from_fn(qbittorrent::auth::require_same_origin))
                .with_state(state.clone());
            main_router = main_router.nest("/web/", webui_router);
            main_router = main_router.route("/web", get(|| async { Redirect::permanent("./web/") }))
        }

        if state.opts.enable_qbittorrent_api {
            info!("Enabling qBittorrent Web API compatibility routes");
            main_router = main_router.nest("/api/v2", qbittorrent::make_api_router(state.clone()));
        }

        let app = main_router
            .layer(axum::middleware::from_fn(
                qbittorrent::auth::require_same_origin,
            ))
            .layer(axum::middleware::from_fn(security_headers))
            .layer(cors_layer)
            .layer(
                tower_http::trace::TraceLayer::new_for_http()
                    .make_span_with(|req: &Request| {
                        let method = req.method();
                        // Query strings can contain magnet links and private tracker passkeys.
                        let path = req.uri().path();
                        if let Some(ConnectInfo(addr)) = req
                            .extensions()
                            .get::<ConnectInfo<librqbit_dualstack_sockets::WrappedSocketAddr>>()
                        {
                            debug_span!("request", %method, %path, addr=%addr.0)
                        } else {
                            debug_span!("request", %method, %path)
                        }
                    })
                    // Never log raw request headers: they may contain credentials or cookies.
                    // Response headers may contain authentication cookies.
                    .on_response(DefaultOnResponse::new())
                    .on_failure({
                        let mut default = DefaultOnFailure::new();
                        move |failure_class, latency, span: &Span| match failure_class {
                            tower_http::classify::ServerErrorsFailureClass::StatusCode(
                                StatusCode::NOT_IMPLEMENTED,
                            ) => {}
                            _ => default.on_failure(failure_class, latency, span),
                        }
                    }),
            )
            .into_make_service_with_connect_info::<librqbit_dualstack_sockets::WrappedSocketAddr>();

        async move {
            axum::serve(listener, app)
                .await
                .context("error running HTTP API")
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::{HttpApi, HttpApiOptions, compile_cors_allow_regex};
    use crate::{Api, Session, SessionOptions};

    const TEST_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn bearer_credentials_are_exact_unambiguous_and_fail_closed() {
        use http::{HeaderMap, HeaderValue, header::AUTHORIZATION};
        let mut headers = HeaderMap::new();
        assert!(!super::credentials_match_bearer(Some(TEST_KEY), &headers));
        for value in [
            "Bearer ".to_owned(),
            "Bearer short".to_owned(),
            format!("Bearer {TEST_KEY}x"),
            format!("Bearer {}", &TEST_KEY[..63]),
            format!("Bearer  {TEST_KEY}"),
            format!("Basic {TEST_KEY}"),
            format!("Bearer {TEST_KEY},other"),
            format!("Bearer {TEST_KEY}="),
        ] {
            headers.insert(AUTHORIZATION, HeaderValue::from_str(&value).unwrap());
            assert!(!super::credentials_match_bearer(Some(TEST_KEY), &headers));
        }
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("bEaReR {TEST_KEY}")).unwrap(),
        );
        assert!(super::credentials_match_bearer(Some(TEST_KEY), &headers));
        assert!(!super::credentials_match_bearer(None, &headers));
        assert!(!super::credentials_match_bearer(Some(""), &headers));
        headers.append(AUTHORIZATION, HeaderValue::from_static("Bearer wrong"));
        assert!(!super::credentials_match_bearer(Some(TEST_KEY), &headers));
        assert!(!super::credentials_match_basic(None, &HeaderMap::new()));
    }

    #[tokio::test]
    async fn every_listener_requires_credentials_and_rejects_weak_tokens() {
        let downloads = tempfile::tempdir().unwrap();
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        for read_only in [false, true] {
            for ip in [Ipv4Addr::LOCALHOST, Ipv4Addr::UNSPECIFIED] {
                for credentials in [
                    None,
                    Some((String::new(), "secret".into())),
                    Some(("user".into(), String::new())),
                ] {
                    let listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
                        (ip, 0).into(),
                        Default::default(),
                    )
                    .unwrap();
                    let error = HttpApi::new(
                        Api::new(session.clone(), None, None),
                        Some(HttpApiOptions {
                            read_only,
                            basic_auth: credentials,
                            ..Default::default()
                        }),
                    )
                    .make_http_api_and_run(listener, None)
                    .await
                    .unwrap_err();
                    assert!(
                        error
                            .to_string()
                            .contains("requires a non-empty username and password")
                    );
                }
            }
        }
        for key in [
            "".to_owned(),
            "short".to_owned(),
            "a=".repeat(32),
            "a".repeat(257),
            format!("{} ", "a".repeat(32)),
            format!("a{}", "=".repeat(32)),
        ] {
            let listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
                (Ipv4Addr::LOCALHOST, 0).into(),
                Default::default(),
            )
            .unwrap();
            let error = HttpApi::new(
                Api::new(session.clone(), None, None),
                Some(HttpApiOptions {
                    basic_auth: Some(("user".into(), "secret".into())),
                    qbittorrent_api_key: Some(key),
                    ..Default::default()
                }),
            )
            .make_http_api_and_run(listener, None)
            .await
            .unwrap_err();
            assert!(error.to_string().contains("API key must contain"));
        }
    }

    #[tokio::test]
    async fn protected_routes_include_upnp_and_reject_token_scope_and_query_bypasses() {
        let downloads = tempfile::tempdir().unwrap();
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let html_path = downloads.path().join("untrusted.html");
        std::fs::write(&html_path, "<script>fetch('/torrents')</script>").unwrap();
        let torrent = crate::create_torrent(
            &html_path,
            crate::CreateTorrentOptions::default(),
            &crate::spawn_utils::BlockingSpawner::new(1),
        )
        .await
        .unwrap();
        let handle = session
            .add_torrent(
                crate::AddTorrent::TorrentFileBytes(torrent.as_bytes().unwrap()),
                Some(crate::AddTorrentOptions {
                    overwrite: true,
                    paused: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
            .into_handle()
            .unwrap();
        handle.wait_until_initialized().await.unwrap();
        let stream_path = format!("/torrents/{}/stream/0", handle.info_hash().as_string());
        let listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Default::default(),
        )
        .unwrap();
        let base = format!("http://{}", listener.bind_addr());
        let upnp = axum::Router::new().route(
            "/description.xml",
            axum::routing::get(|| async { "private UPnP data" }),
        );
        let server = tokio::spawn(
            HttpApi::new(
                Api::new(session, None, None),
                Some(HttpApiOptions {
                    basic_auth: Some(("user".into(), "secret".into())),
                    enable_qbittorrent_api: true,
                    qbittorrent_api_key: Some(TEST_KEY.into()),
                    ..Default::default()
                }),
            )
            .make_http_api_and_run(listener, Some(upnp)),
        );
        let client = reqwest::Client::new();
        for path in [
            "/",
            "/stats",
            "/torrents",
            "/stream_logs",
            "/dht/table",
            "/upnp/description.xml",
            "/torrents/0/stream/0",
        ] {
            let response = client.get(format!("{base}{path}")).send().await.unwrap();
            assert_eq!(
                response.status(),
                reqwest::StatusCode::UNAUTHORIZED,
                "{path}"
            );
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
        let response = client
            .get(format!(
                "{base}/api/v2/app/version?token={TEST_KEY}&api_key={TEST_KEY}&SID={TEST_KEY}"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        let response = client
            .get(format!("{base}/api/v2/app/version"))
            .bearer_auth(TEST_KEY)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let response = client
            .get(format!("{base}/stats"))
            .bearer_auth(TEST_KEY)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
        let response = client
            .get(format!("{base}/upnp/description.xml"))
            .basic_auth("user", Some("secret"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        for path in [
            "/stats",
            "/torrents",
            "/upnp/description.xml",
            "/api/v2/app/version",
        ] {
            let response = client
                .get(format!("{base}{path}"))
                .basic_auth("user", Some("secret"))
                .header("Origin", "https://attacker.invalid")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        }
        let response = client
            .get(format!("{base}{stream_path}"))
            .basic_auth("user", Some("secret"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"],
            "application/octet-stream"
        );
        assert_eq!(response.headers()["content-disposition"], "attachment");
        assert!(
            response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("sandbox")
        );
        assert_eq!(
            response.text().await.unwrap(),
            "<script>fetch('/torrents')</script>"
        );
        let response = client
            .get(format!("{base}{stream_path}"))
            .basic_auth("user", Some("secret"))
            .header("Range", "bytes=0-7")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert!(
            response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("sandbox")
        );
        assert_eq!(response.bytes().await.unwrap().as_ref(), b"<script>");
        server.abort();
    }

    #[test]
    fn debug_output_redacts_basic_auth_credentials() {
        let options = HttpApiOptions {
            basic_auth: Some(("sensitive-user".to_owned(), "sensitive-pass".to_owned())),
            qbittorrent_api_key: Some("sensitive-key".to_owned()),
            ..Default::default()
        };

        let debug = format!("{options:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("sensitive-user"));
        assert!(!debug.contains("sensitive-pass"));
        assert!(!debug.contains("sensitive-key"));
    }

    #[test]
    fn custom_cors_regex_must_match_the_entire_origin() {
        let regex = compile_cors_allow_regex(r"https://trusted\.example").unwrap();
        assert!(regex.is_match(b"https://trusted.example"));
        assert!(!regex.is_match(b"https://trusted.example.attacker.invalid"));
    }

    #[tokio::test]
    async fn configured_basic_auth_requires_non_empty_credentials() {
        let downloads = tempfile::tempdir().unwrap();
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Default::default(),
        )
        .unwrap();

        let result = HttpApi::new(
            Api::new(session, None, None),
            Some(HttpApiOptions {
                basic_auth: Some((String::new(), String::new())),
                ..Default::default()
            }),
        )
        .make_http_api_and_run(listener, None)
        .await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("requires a non-empty username and password")
        );
    }

    #[tokio::test]
    async fn writable_api_rejects_cross_origin_and_dns_rebinding_requests() {
        let downloads = tempfile::tempdir().unwrap();
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Default::default(),
        )
        .unwrap();
        let address = listener.bind_addr();
        let server = HttpApi::new(
            Api::new(session, None, None),
            Some(HttpApiOptions {
                basic_auth: Some(("test-user".into(), "secret".into())),
                ..Default::default()
            }),
        )
        .make_http_api_and_run(listener, None);
        let task = tokio::spawn(async move { server.await.unwrap() });
        let client = reqwest::Client::new();

        let response = client
            .post(format!("http://{address}/torrents/limits"))
            .basic_auth("test-user", Some("secret"))
            .header("Origin", "https://attacker.example")
            .json(&serde_json::json!({"upload_bps": null, "download_bps": null}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);

        let response = client
            .post(format!("http://{address}/torrents/limits"))
            .basic_auth("test-user", Some("secret"))
            .header("Host", "attacker.example")
            .header("Origin", "http://different-attacker.example")
            .json(&serde_json::json!({"upload_bps": null, "download_bps": null}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);

        let response = client
            .post(format!("http://{address}/torrents/limits"))
            .basic_auth("test-user", Some("secret"))
            .json(&serde_json::json!({"upload_bps": null, "download_bps": null}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        task.abort();
    }

    #[tokio::test]
    async fn read_only_api_requires_authentication_and_rejects_log_changes() {
        let downloads = tempfile::tempdir().unwrap();
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Default::default(),
        )
        .unwrap();
        let address = listener.bind_addr();
        let server = HttpApi::new(
            Api::new(session, None, None),
            Some(HttpApiOptions {
                read_only: true,
                basic_auth: Some(("test-user".into(), "secret".into())),
                ..Default::default()
            }),
        )
        .make_http_api_and_run(listener, None);
        let task = tokio::spawn(async move { server.await.unwrap() });
        let client = reqwest::Client::new();

        let response = client
            .get(format!("http://{address}/torrents"))
            .header("Host", "attacker.example")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

        let response = client
            .get(format!("http://{address}/torrents"))
            .basic_auth("test-user", Some("secret"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let response = client
            .post(format!("http://{address}/rust_log"))
            .body("trace")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        task.abort();
    }

    #[cfg(feature = "webui")]
    #[tokio::test]
    async fn web_login_uses_a_session_and_keeps_basic_auth_compatible() {
        let public_ip_listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Default::default(),
        )
        .unwrap();
        let public_ip_address = public_ip_listener.bind_addr();
        let public_ip_task = tokio::spawn(async move {
            axum::serve(
                public_ip_listener,
                axum::Router::new().route("/", axum::routing::get(|| async { "203.0.113.10" })),
            )
            .await
            .unwrap()
        });

        let downloads = tempfile::tempdir().unwrap();
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Default::default(),
        )
        .unwrap();
        let address = listener.bind_addr();
        let mut http_api = HttpApi::new(
            Api::new(session, None, None),
            Some(HttpApiOptions {
                basic_auth: Some(("test-user".to_owned(), "secret".to_owned())),
                enable_qbittorrent_api: true,
                ..Default::default()
            }),
        );
        http_api.public_ip_lookup_url = format!("http://{public_ip_address}");
        let server = http_api.make_http_api_and_run(listener, None);
        let task = tokio::spawn(async move { server.await.unwrap() });
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let base = format!("http://{address}");

        let response = client
            .get(format!("{base}/"))
            .header("Accept", "text/html")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
        assert!(response.headers().get("WWW-Authenticate").is_none());

        let response = client.get(format!("{base}/web/")).send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(response.headers().get("WWW-Authenticate").is_none());

        let status: serde_json::Value = client
            .get(format!("{base}/web/auth/status"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(status["authenticated"], false);
        assert_eq!(status["authentication_required"], true);
        assert!(status.get("public_ip").is_none());

        let response = client
            .get(format!("{base}/web/public-ip"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

        let response = client
            .get(format!("{base}/"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

        let response = client
            .post(format!("{base}/web/auth/login"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("username=test-user&password=wrong")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert!(response.headers().get("WWW-Authenticate").is_none());

        let response = client
            .post(format!("{base}/web/auth/login"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("username=test-user&password=secret")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
        assert!(
            response.headers()["Set-Cookie"]
                .to_str()
                .unwrap()
                .contains("; Secure")
        );
        let cookie = response
            .headers()
            .get("Set-Cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();

        let response = client
            .get(format!("{base}/"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let status: serde_json::Value = client
            .get(format!("{base}/web/auth/status"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(status["authenticated"], true);
        assert!(status.get("public_ip").is_none());

        let public_ip: serde_json::Value = client
            .get(format!("{base}/web/public-ip"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(public_ip["public_ip"], "203.0.113.10");

        let response = client
            .post(format!("{base}/web/auth/logout"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
        assert_eq!(
            response.headers().get("Set-Cookie").unwrap(),
            "SID=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"
        );
        assert!(response.bytes().await.unwrap().is_empty());

        let status: serde_json::Value = client
            .get(format!("{base}/web/auth/status"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(status["authenticated"], false);
        assert!(status.get("public_ip").is_none());

        let response = client
            .get(format!("{base}/stats"))
            .header("Cookie", &cookie)
            .header("X-Rqbit-WebUI", "1")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert!(response.headers().get("WWW-Authenticate").is_none());

        let response = client
            .get(format!("{base}/"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

        // The two revoked-cookie requests above also count as failed credentials.
        for _ in 0..3 {
            let response = client
                .get(format!("{base}/stats"))
                .basic_auth("test-user", Some("wrong"))
                .header("X-Rqbit-WebUI", "1")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
        }

        let response = client
            .get(format!("{base}/api/v2/app/version"))
            .basic_auth("test-user", Some("secret"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);

        task.abort();
        public_ip_task.abort();
    }
}
