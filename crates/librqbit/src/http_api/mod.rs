use anyhow::Context;
use axum::extract::{ConnectInfo, Request};
use axum::middleware::Next;
use axum::response::IntoResponse;
#[cfg(any(feature = "webui", feature = "prometheus"))]
use axum::routing::get;
use base64::Engine;
use futures::FutureExt;
use futures::future::BoxFuture;
use http::{HeaderMap, StatusCode, header::HOST};
use librqbit_dualstack_sockets::TcpListener;
use sha1w::{ISha256, Sha256};
use std::sync::Arc;
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
}

#[derive(Default)]
pub struct HttpApiOptions {
    pub read_only: bool,
    pub basic_auth: Option<(String, String)>,
    // Allow creating torrents via API.
    pub allow_create: bool,
    /// Maximum upload body size.
    pub max_upload_body_size: Option<usize>,
    /// Expose the Servarr-focused qBittorrent Web API compatibility routes.
    pub enable_qbittorrent_api: bool,
    #[cfg(feature = "prometheus")]
    pub prometheus_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
}

impl std::fmt::Debug for HttpApiOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = formatter.debug_struct("HttpApiOptions");
        debug
            .field("read_only", &self.read_only)
            .field(
                "basic_auth",
                &self.basic_auth.as_ref().map(|_| "<redacted>"),
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
        _ => return Ok(next.run(request).await),
    };
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
        if headers.get("Authorization").is_none() {
            return Ok((
                StatusCode::UNAUTHORIZED,
                [("WWW-Authenticate", "Basic realm=\"API\"")],
            )
                .into_response());
        }
        sessions.record_login_failure(client_ip);
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
        return true;
    };
    let Some((user, pass)) = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Basic "))
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

fn constant_time_eq(expected: &[u8; 32], actual: &[u8; 32]) -> bool {
    let mut difference = 0_u8;
    for index in 0..expected.len() {
        difference |= expected[index] ^ actual[index];
    }
    difference == 0
}

async fn require_loopback_host(
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> impl IntoResponse {
    let is_loopback = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|host| host.parse::<http::uri::Authority>().ok())
        .is_some_and(|authority| {
            let host = authority.host();
            host.eq_ignore_ascii_case("localhost")
                || host.eq_ignore_ascii_case("localhost.")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        });
    if !is_loopback {
        return (StatusCode::FORBIDDEN, "Invalid Host header").into_response();
    }
    next.run(request).await
}

impl HttpApi {
    pub fn new(api: Api, opts: Option<HttpApiOptions>) -> Self {
        Self {
            api,
            opts: opts.unwrap_or_default(),
            qbittorrent_sessions: Default::default(),
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
            .is_some_and(|(user, pass)| !user.is_empty() && !pass.is_empty());
        let enforce_loopback_host = !self.opts.read_only
            && listener.bind_addr().ip().is_loopback()
            && !credentials_configured;
        if self.opts.enable_qbittorrent_api && !credentials_configured {
            return async {
                anyhow::bail!(
                    "the qBittorrent compatibility API requires a non-empty username and password"
                )
            }
            .boxed();
        }
        if !self.opts.read_only
            && !listener.bind_addr().ip().is_loopback()
            && !credentials_configured
        {
            return async {
                anyhow::bail!(
                    "a writable HTTP API on a non-loopback address requires a non-empty username and password"
                )
            }
            .boxed();
        }

        #[cfg(feature = "prometheus")]
        let mut prometheus_handle = self.opts.prometheus_handle.take();

        let state = Arc::new(self);

        let mut main_router = handlers::make_api_router(state.clone());

        #[cfg(feature = "webui")]
        {
            use axum::response::Redirect;

            let webui_router = webui::make_webui_router();
            main_router = main_router.nest("/web/", webui_router);
            main_router = main_router.route("/web", get(|| async { Redirect::permanent("./web/") }))
        }

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
                .and_then(|value| regex::bytes::Regex::new(&value).ok());

            tower_http::cors::CorsLayer::default()
                .allow_origin(AllowOrigin::predicate(move |v, _| {
                    ALLOWED_ORIGINS.contains(&v.as_bytes())
                        || allow_regex
                            .as_ref()
                            .map(move |r| r.is_match(v.as_bytes()))
                            .unwrap_or(false)
                }))
                .allow_headers(AllowHeaders::any())
        };

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

        if state.opts.enable_qbittorrent_api {
            info!("Enabling qBittorrent Web API compatibility routes");
            main_router = main_router.nest("/api/v2", qbittorrent::make_api_router(state.clone()));
        }

        if let Some(upnp_router) = upnp_router {
            main_router = main_router.nest("/upnp", upnp_router);
        }

        // An unauthenticated loopback API otherwise remains reachable through
        // DNS rebinding, where an attacker-controlled hostname resolves to
        // 127.0.0.1 and makes Origin and Host appear to match.
        if enforce_loopback_host {
            main_router = main_router.layer(axum::middleware::from_fn(require_loopback_host));
        }

        let app = main_router
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

    use super::{HttpApi, HttpApiOptions};
    use crate::{Api, Session, SessionOptions};

    #[test]
    fn debug_output_redacts_basic_auth_credentials() {
        let options = HttpApiOptions {
            basic_auth: Some(("sensitive-user".to_owned(), "sensitive-pass".to_owned())),
            ..Default::default()
        };

        let debug = format!("{options:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("sensitive-user"));
        assert!(!debug.contains("sensitive-pass"));
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
        let server =
            HttpApi::new(Api::new(session, None, None), None).make_http_api_and_run(listener, None);
        let task = tokio::spawn(async move { server.await.unwrap() });
        let client = reqwest::Client::new();

        let response = client
            .post(format!("http://{address}/torrents/limits"))
            .header("Origin", "https://attacker.example")
            .json(&serde_json::json!({"upload_bps": null, "download_bps": null}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);

        let response = client
            .post(format!("http://{address}/torrents/limits"))
            .header("Host", "attacker.example")
            .header("Origin", "http://attacker.example")
            .json(&serde_json::json!({"upload_bps": null, "download_bps": null}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);

        let response = client
            .post(format!("http://{address}/torrents/limits"))
            .json(&serde_json::json!({"upload_bps": null, "download_bps": null}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        task.abort();
    }
}
