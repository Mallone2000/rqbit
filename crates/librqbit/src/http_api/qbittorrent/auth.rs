use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, Instant},
};

use axum::{
    Form,
    extract::{ConnectInfo, FromRequest, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use http::{
    HeaderMap, HeaderValue, StatusCode,
    header::{AUTHORIZATION, CACHE_CONTROL, HOST, ORIGIN, REFERER, SET_COOKIE},
};
use librqbit_dualstack_sockets::WrappedSocketAddr;
use parking_lot::Mutex;
use serde::Deserialize;
use subtle::ConstantTimeEq;

use super::ApiState;
use crate::http_api::credentials_match_basic;

const SESSION_TTL: Duration = Duration::from_secs(60 * 60);
const MAX_SESSIONS: usize = 256;
const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);
const AUTH_BAN_DURATION: Duration = Duration::from_secs(60 * 60);
const MAX_AUTH_FAILURES: u8 = 5;
const MAX_TRACKED_CLIENTS: usize = 1024;
const MAX_PUBLIC_IP_RESPONSE_SIZE: usize = 64;

struct AuthFailures {
    count: u8,
    first_failure: Instant,
    last_failure: Instant,
    banned_until: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct Sessions {
    entries: Mutex<HashMap<[u8; 32], Instant>>,
    auth_failures: Mutex<HashMap<Option<IpAddr>, AuthFailures>>,
}

impl Sessions {
    fn issue(&self) -> String {
        let now = Instant::now();
        let mut entries = self.entries.lock();
        entries.retain(|_, expires| *expires > now);
        if entries.len() >= MAX_SESSIONS
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, expires)| **expires)
                .map(|(sid, _)| *sid)
        {
            entries.remove(&oldest);
        }
        let sid = hex::encode(rand::random::<[u8; 32]>());
        entries.insert(crate::http_api::credential_digest(&sid), now + SESSION_TTL);
        sid
    }

    fn is_valid(&self, sid: &str) -> bool {
        let now = Instant::now();
        let mut entries = self.entries.lock();
        entries.retain(|_, expires| *expires > now);
        if sid.len() != 64 || !sid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return false;
        }
        let supplied = crate::http_api::credential_digest(sid);
        // Scan the bounded store without secret-dependent early returns. Raw session
        // credentials are never retained in the server's session records.
        let mut found = subtle::Choice::from(0);
        for expected in entries.keys() {
            found |= expected.ct_eq(&supplied);
        }
        bool::from(found)
    }

    pub(crate) fn authenticates(&self, headers: &HeaderMap) -> bool {
        sid_from_headers(headers).is_some_and(|sid| self.is_valid(sid))
    }

    fn revoke(&self, sid: &str) {
        self.entries
            .lock()
            .remove(&crate::http_api::credential_digest(sid));
    }

    pub(crate) fn login_is_allowed(&self, client_ip: Option<IpAddr>) -> bool {
        let now = Instant::now();
        let mut failures = self.auth_failures.lock();
        failures.retain(|_, entry| {
            entry.banned_until.is_some_and(|until| until > now)
                || now.duration_since(entry.last_failure) <= AUTH_FAILURE_WINDOW
        });
        if failures.len() >= MAX_TRACKED_CLIENTS && !failures.contains_key(&client_ip) {
            return false;
        }
        !failures
            .get(&client_ip)
            .and_then(|entry| entry.banned_until)
            .is_some_and(|until| until > now)
    }

    pub(crate) fn record_login_failure(&self, client_ip: Option<IpAddr>) {
        let now = Instant::now();
        let mut failures = self.auth_failures.lock();
        failures.retain(|_, entry| {
            entry.banned_until.is_some_and(|until| until > now)
                || now.duration_since(entry.last_failure) <= AUTH_FAILURE_WINDOW
        });
        // Never evict an active ban to make room for a new address.
        if failures.len() >= MAX_TRACKED_CLIENTS && !failures.contains_key(&client_ip) {
            return;
        }

        let entry = failures.entry(client_ip).or_insert(AuthFailures {
            count: 0,
            first_failure: now,
            last_failure: now,
            banned_until: None,
        });
        if now.duration_since(entry.first_failure) > AUTH_FAILURE_WINDOW {
            entry.count = 0;
            entry.first_failure = now;
            entry.banned_until = None;
        }
        entry.count = entry.count.saturating_add(1);
        entry.last_failure = now;
        if entry.count >= MAX_AUTH_FAILURES {
            entry.banned_until = Some(now + AUTH_BAN_DURATION);
        }
    }

    pub(crate) fn clear_login_failures(&self, client_ip: Option<IpAddr>) {
        self.auth_failures.lock().remove(&client_ip);
    }
}

#[derive(Deserialize)]
pub(super) struct Login {
    username: String,
    password: String,
}

pub(super) async fn login(State(state): State<ApiState>, request: Request) -> Response {
    let client_ip = request
        .extensions()
        .get::<ConnectInfo<WrappedSocketAddr>>()
        .map(|ConnectInfo(address)| address.0.ip());
    let Some((expected_user, expected_pass)) = state.opts.basic_auth.as_ref() else {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    };

    if !state.qbittorrent_sessions.login_is_allowed(client_ip) {
        return (StatusCode::TOO_MANY_REQUESTS, "Fails.").into_response();
    }
    let Form(login) = match Form::<Login>::from_request(request, &state).await {
        Ok(login) => login,
        Err(error) => return error.into_response(),
    };

    // Reading a body yields. A ban may have been established by another request
    // while this form was in flight; do not let queued logins bypass it.
    if !state.qbittorrent_sessions.login_is_allowed(client_ip) {
        return (StatusCode::TOO_MANY_REQUESTS, "Fails.").into_response();
    }

    if !crate::http_api::constant_time_credentials_match(
        expected_user,
        expected_pass,
        &login.username,
        &login.password,
    ) {
        state.qbittorrent_sessions.record_login_failure(client_ip);
        // Web API 2.9.2 predates the 2.14 change to a 401 response.
        return (StatusCode::OK, "Fails.").into_response();
    }

    state.qbittorrent_sessions.clear_login_failures(client_ip);
    let sid = state.qbittorrent_sessions.issue();
    let mut response = (StatusCode::OK, "Ok.").into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&format!(
            "SID={sid}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
            SESSION_TTL.as_secs(),
            if state.opts.insecure_session_cookies {
                ""
            } else {
                "; Secure"
            }
        ))
        .expect("hex SID always makes a valid cookie"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(in crate::http_api) async fn web_status(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Response {
    let authenticated = state.qbittorrent_sessions.authenticates(&headers);
    let mut response = axum::Json(serde_json::json!({
        "authenticated": authenticated,
        "authentication_required": state.opts.basic_auth.is_some(),
    }))
    .into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(in crate::http_api) async fn web_public_ip(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Response {
    if !state.qbittorrent_sessions.authenticates(&headers) {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let response = match state
        .public_ip_client
        .get(&state.public_ip_lookup_url)
        .timeout(Duration::from_secs(3))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response,
        _ => {
            return (StatusCode::BAD_GATEWAY, "Unable to determine public IP").into_response();
        }
    };
    let public_ip = match parse_public_ip_response(response).await {
        Ok(public_ip) => public_ip,
        Err(_) => return (StatusCode::BAD_GATEWAY, "Invalid public IP response").into_response(),
    };

    axum::Json(serde_json::json!({ "public_ip": public_ip })).into_response()
}

async fn parse_public_ip_response(mut response: reqwest::Response) -> Result<IpAddr, ()> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PUBLIC_IP_RESPONSE_SIZE as u64)
    {
        return Err(());
    }
    let mut body = Vec::with_capacity(MAX_PUBLIC_IP_RESPONSE_SIZE);
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
        if body.len().saturating_add(chunk.len()) > MAX_PUBLIC_IP_RESPONSE_SIZE {
            return Err(());
        }
        body.extend_from_slice(&chunk);
    }
    std::str::from_utf8(&body)
        .map_err(|_| ())?
        .trim()
        .parse()
        .map_err(|_| ())
}

pub(in crate::http_api) async fn web_login(
    State(state): State<ApiState>,
    request: Request,
) -> Response {
    let client_ip = request
        .extensions()
        .get::<ConnectInfo<WrappedSocketAddr>>()
        .map(|ConnectInfo(address)| address.0.ip());
    let Some((expected_user, expected_pass)) = state.opts.basic_auth.as_ref() else {
        return StatusCode::FORBIDDEN.into_response();
    };

    if !state.qbittorrent_sessions.login_is_allowed(client_ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "Too many authentication failures",
        )
            .into_response();
    }
    let Form(login) = match Form::<Login>::from_request(request, &state).await {
        Ok(login) => login,
        Err(error) => return error.into_response(),
    };

    // Reading a body yields. A ban may have been established by another request
    // while this form was in flight; do not let queued logins bypass it.
    if !state.qbittorrent_sessions.login_is_allowed(client_ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "Too many authentication failures",
        )
            .into_response();
    }

    if !crate::http_api::constant_time_credentials_match(
        expected_user,
        expected_pass,
        &login.username,
        &login.password,
    ) {
        state.qbittorrent_sessions.record_login_failure(client_ip);
        return (StatusCode::UNAUTHORIZED, "Invalid username or password").into_response();
    }

    state.qbittorrent_sessions.clear_login_failures(client_ip);
    let sid = state.qbittorrent_sessions.issue();
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&format!(
            "SID={sid}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
            SESSION_TTL.as_secs(),
            if state.opts.insecure_session_cookies {
                ""
            } else {
                "; Secure"
            }
        ))
        .expect("hex SID always makes a valid cookie"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(in crate::http_api) async fn logout(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Response {
    logout_response(&state, &headers, (StatusCode::OK, "Ok.").into_response())
}

pub(in crate::http_api) async fn web_logout(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Response {
    logout_response(&state, &headers, StatusCode::NO_CONTENT.into_response())
}

fn logout_response(state: &ApiState, headers: &HeaderMap, mut response: Response) -> Response {
    if let Some(sid) = sid_from_headers(headers) {
        state.qbittorrent_sessions.revoke(sid);
    }
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_static("SID=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(super) async fn require_auth(
    State(state): State<ApiState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    if state.opts.basic_auth.is_none() {
        return (StatusCode::FORBIDDEN, "Forbidden").into_response();
    }

    if state.qbittorrent_sessions.authenticates(&headers) {
        return next.run(request).await;
    }

    let client_ip = request
        .extensions()
        .get::<ConnectInfo<WrappedSocketAddr>>()
        .map(|ConnectInfo(address)| address.0.ip());
    if !state.qbittorrent_sessions.login_is_allowed(client_ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "Too many authentication failures",
        )
            .into_response();
    }
    if credentials_match_basic(state.opts.basic_auth.as_ref(), &headers) {
        state.qbittorrent_sessions.clear_login_failures(client_ip);
        return next.run(request).await;
    }
    if crate::http_api::credentials_match_bearer(
        state.opts.qbittorrent_api_key.as_deref(),
        &headers,
    ) {
        state.qbittorrent_sessions.clear_login_failures(client_ip);
        return next.run(request).await;
    }

    if headers.contains_key(AUTHORIZATION) || sid_from_headers(&headers).is_some() {
        state.qbittorrent_sessions.record_login_failure(client_ip);
    }

    (StatusCode::FORBIDDEN, "Forbidden").into_response()
}

pub(in crate::http_api) async fn require_same_origin(
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    if request_source_matches_host(&headers) {
        next.run(request).await
    } else {
        (StatusCode::FORBIDDEN, "Cross-origin requests are forbidden").into_response()
    }
}

fn request_source_matches_host(headers: &HeaderMap) -> bool {
    if headers.get_all(ORIGIN).iter().count() > 1
        || headers.get_all(REFERER).iter().count() > 1
        || headers.get_all(HOST).iter().count() > 1
    {
        return false;
    }
    // Fetch Metadata covers browser requests that omit Origin and Referer.
    if headers
        .get("sec-fetch-site")
        .is_some_and(|site| site == "cross-site")
    {
        return false;
    }
    let source = headers.get(ORIGIN).or_else(|| headers.get(REFERER));
    let Some(source) = source else {
        return true;
    };
    let Ok(source) = source.to_str() else {
        return false;
    };
    let Some(host) = headers.get(HOST).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Some(source) = source.parse::<http::Uri>().ok() else {
        return false;
    };
    let Some(source_authority) = source.authority() else {
        return false;
    };
    if !matches!(source.scheme_str(), Some("http" | "https" | "tauri")) {
        return false;
    }
    if source_authority.as_str().eq_ignore_ascii_case(host) {
        return true;
    }

    // The development Web UI and Tauri use a different origin from the API.
    // Only trust those specific UI origins. Treating every loopback port as
    // trusted would let an unrelated local web app submit authenticated
    // requests because SameSite cookies do not isolate ports.
    let Ok(host_authority) = host.parse::<http::uri::Authority>() else {
        return false;
    };
    if !is_loopback_host(host_authority.host()) {
        return false;
    }

    match source.scheme_str() {
        Some(scheme) if scheme.eq_ignore_ascii_case("tauri") => {
            source_authority.as_str().eq_ignore_ascii_case("localhost")
        }
        Some(scheme) if cfg!(debug_assertions) && scheme.eq_ignore_ascii_case("http") => {
            let source_host = source_authority.host();
            let source_port = source_authority.port_u16();
            (source_host.eq_ignore_ascii_case("localhost")
                && matches!(source_port, Some(3031 | 1420)))
                || (source_host == "127.0.0.1" && source_port == Some(3031))
        }
        _ => false,
    }
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("localhost.")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn sid_from_headers(headers: &HeaderMap) -> Option<&str> {
    let mut sid = None;
    for header in headers.get_all(http::header::COOKIE) {
        for cookie in header.to_str().ok()?.split(';').map(str::trim) {
            if let Some(value) = cookie.strip_prefix("SID=") {
                if sid.is_some() {
                    return None;
                }
                sid = Some(value);
            }
        }
    }
    sid
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use http::{HeaderMap, HeaderValue};

    use super::{
        MAX_AUTH_FAILURES, MAX_PUBLIC_IP_RESPONSE_SIZE, Sessions, parse_public_ip_response,
        request_source_matches_host,
    };

    #[test]
    fn repeated_authentication_failures_are_rate_limited() {
        let sessions = Sessions::default();
        let client_ip = Some(Ipv4Addr::LOCALHOST.into());

        for _ in 1..MAX_AUTH_FAILURES {
            sessions.record_login_failure(client_ip);
            assert!(sessions.login_is_allowed(client_ip));
        }
        sessions.record_login_failure(client_ip);
        assert!(!sessions.login_is_allowed(client_ip));

        sessions.clear_login_failures(client_ip);
        assert!(sessions.login_is_allowed(client_ip));
    }

    #[test]
    fn browser_request_source_must_match_host() {
        let mut headers = HeaderMap::new();
        headers.insert(super::HOST, HeaderValue::from_static("localhost:3030"));
        headers.insert(
            super::ORIGIN,
            HeaderValue::from_static("http://localhost:3030"),
        );
        assert!(request_source_matches_host(&headers));

        headers.insert(
            super::ORIGIN,
            HeaderValue::from_str("https://attacker.example").unwrap(),
        );
        assert!(!request_source_matches_host(&headers));

        for origin in ["ftp://localhost:3030", "null"] {
            headers.insert(super::ORIGIN, HeaderValue::from_str(origin).unwrap());
            assert!(!request_source_matches_host(&headers));
        }
        headers.remove(super::ORIGIN);
        headers.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        assert!(!request_source_matches_host(&headers));
        headers.remove("sec-fetch-site");
        headers.insert(super::ORIGIN, HeaderValue::from_bytes(b"\xff").unwrap());
        assert!(!request_source_matches_host(&headers));

        headers.insert(
            super::ORIGIN,
            HeaderValue::from_static("http://localhost:3031"),
        );
        assert!(request_source_matches_host(&headers));

        headers.insert(super::ORIGIN, HeaderValue::from_static("tauri://localhost"));
        assert!(request_source_matches_host(&headers));

        headers.insert(
            super::ORIGIN,
            HeaderValue::from_static("http://localhost:9999"),
        );
        assert!(!request_source_matches_host(&headers));

        headers.insert(
            super::ORIGIN,
            HeaderValue::from_static("http://127.0.0.1:9999"),
        );
        assert!(!request_source_matches_host(&headers));
    }

    #[tokio::test]
    async fn cookie_transport_requires_explicit_insecure_opt_in() {
        use crate::{
            Api, Session, SessionOptions,
            http_api::{HttpApi, HttpApiOptions},
        };
        use axum::{body::Body, extract::State};
        use std::sync::Arc;
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
        for web in [false, true] {
            for insecure in [false, true] {
                let state = Arc::new(HttpApi::new(
                    Api::new(session.clone(), None, None),
                    Some(HttpApiOptions {
                        basic_auth: Some(("user".into(), "secret".into())),
                        insecure_session_cookies: insecure,
                        ..Default::default()
                    }),
                ));
                let request = http::Request::builder()
                    .method("POST")
                    .header(
                        http::header::CONTENT_TYPE,
                        "application/x-www-form-urlencoded",
                    )
                    .header("x-forwarded-proto", "http")
                    .body(Body::from("username=user&password=secret"))
                    .unwrap();
                let response = if web {
                    super::web_login(State(state.clone()), request).await
                } else {
                    super::login(State(state.clone()), request).await
                };
                assert!(response.status().is_success());
                let cookie = response.headers()[super::SET_COOKIE].to_str().unwrap();
                assert_eq!(cookie.contains("; Secure"), !insecure);
                assert!(cookie.contains("; HttpOnly; SameSite=Strict;"));
                let mut headers = HeaderMap::new();
                headers.insert(
                    http::header::COOKIE,
                    HeaderValue::from_str(cookie.split(';').next().unwrap()).unwrap(),
                );
                assert!(state.qbittorrent_sessions.authenticates(&headers));
                let response = super::logout(State(state.clone()), headers.clone()).await;
                assert!(response.status().is_success());
                assert!(!state.qbittorrent_sessions.authenticates(&headers));
            }
        }
    }

    #[tokio::test]
    async fn in_flight_login_cannot_bypass_a_new_ban() {
        use crate::{
            Api, Session, SessionOptions,
            http_api::{HttpApi, HttpApiOptions},
        };
        use axum::{body::Body, extract::State};
        use bytes::Bytes;
        use std::{convert::Infallible, sync::Arc};

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
        for web in [false, true] {
            let state = Arc::new(HttpApi::new(
                Api::new(session.clone(), None, None),
                Some(HttpApiOptions {
                    basic_auth: Some(("user".into(), "secret".into())),
                    ..Default::default()
                }),
            ));
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            let body = Body::from_stream(futures::stream::once(async move {
                started_tx.send(()).unwrap();
                release_rx.await.unwrap();
                Ok::<_, Infallible>(Bytes::from_static(b"username=user&password=secret"))
            }));
            let request = http::Request::builder()
                .method("POST")
                .header(
                    http::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(body)
                .unwrap();
            let task_state = state.clone();
            let task = tokio::spawn(async move {
                if web {
                    super::web_login(State(task_state), request).await
                } else {
                    super::login(State(task_state), request).await
                }
            });
            started_rx.await.unwrap();
            for _ in 0..MAX_AUTH_FAILURES {
                state.qbittorrent_sessions.record_login_failure(None);
            }
            release_tx.send(()).unwrap();
            let response = task.await.unwrap();
            assert_eq!(response.status(), http::StatusCode::TOO_MANY_REQUESTS);
            assert!(!response.headers().contains_key(http::header::SET_COOKIE));
        }
    }

    #[test]
    fn session_tokens_reject_forgery_expiry_and_duplicate_cookies() {
        let sessions = Sessions::default();
        let sid = sessions.issue();
        assert_eq!(sid.len(), 64);
        assert!(sessions.is_valid(&sid));
        for value in [
            String::new(),
            sid[..63].to_owned(),
            format!("{sid}0"),
            "0".repeat(64),
            "z".repeat(64),
        ] {
            assert!(!sessions.is_valid(&value));
        }
        let mut headers = HeaderMap::new();
        headers.append(
            http::header::COOKIE,
            HeaderValue::from_str(&format!("other=1; SID={sid}")).unwrap(),
        );
        assert!(sessions.authenticates(&headers));
        headers.append(http::header::COOKIE, HeaderValue::from_static("SID=forged"));
        assert!(!sessions.authenticates(&headers));
        headers.clear();
        headers.insert(
            http::header::COOKIE,
            HeaderValue::from_str(&format!("SID={sid}; SID={sid}")).unwrap(),
        );
        assert!(!sessions.authenticates(&headers));
        sessions.entries.lock().insert(
            crate::http_api::credential_digest(&sid),
            std::time::Instant::now() - std::time::Duration::from_secs(1),
        );
        assert!(!sessions.is_valid(&sid));
        assert!(!Sessions::default().is_valid(&sid));
    }

    #[test]
    fn login_bans_survive_client_table_exhaustion() {
        let sessions = Sessions::default();
        let target = Some(Ipv4Addr::LOCALHOST.into());
        for _ in 0..MAX_AUTH_FAILURES {
            sessions.record_login_failure(target);
        }
        for index in 0..super::MAX_TRACKED_CLIENTS {
            sessions.record_login_failure(Some(
                Ipv4Addr::from(0x0a000001 + u32::try_from(index).unwrap()).into(),
            ));
        }
        assert!(!sessions.login_is_allowed(target));
        assert!(!sessions.login_is_allowed(Some(Ipv4Addr::from(0x0b000001).into())));
    }

    #[test]
    fn revoked_session_is_invalid() {
        let sessions = Sessions::default();
        let sid = sessions.issue();
        assert!(sessions.is_valid(&sid));

        sessions.revoke(&sid);
        assert!(!sessions.is_valid(&sid));
    }

    #[tokio::test]
    async fn public_ip_response_body_is_bounded() {
        let listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
            (Ipv4Addr::LOCALHOST, 0).into(),
            Default::default(),
        )
        .unwrap();
        let address = listener.bind_addr();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                axum::Router::new().route(
                    "/",
                    axum::routing::get(|| async { "x".repeat(MAX_PUBLIC_IP_RESPONSE_SIZE + 1) }),
                ),
            )
            .await
            .unwrap()
        });

        let response = reqwest::get(format!("http://{address}")).await.unwrap();
        assert!(parse_public_ip_response(response).await.is_err());
        task.abort();
    }
}
