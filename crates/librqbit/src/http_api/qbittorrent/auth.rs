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
use uuid::Uuid;

use super::ApiState;
use crate::http_api::credentials_match_basic;

const SESSION_TTL: Duration = Duration::from_secs(60 * 60);
const MAX_SESSIONS: usize = 256;
const AUTH_FAILURE_WINDOW: Duration = Duration::from_secs(15 * 60);
const AUTH_BAN_DURATION: Duration = Duration::from_secs(60 * 60);
const MAX_AUTH_FAILURES: u8 = 5;
const MAX_TRACKED_CLIENTS: usize = 1024;

struct AuthFailures {
    count: u8,
    first_failure: Instant,
    last_failure: Instant,
    banned_until: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct Sessions {
    entries: Mutex<HashMap<String, Instant>>,
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
                .map(|(sid, _)| sid.clone())
        {
            entries.remove(&oldest);
        }
        let sid = Uuid::new_v4().simple().to_string();
        entries.insert(sid.clone(), now + SESSION_TTL);
        sid
    }

    fn is_valid(&self, sid: &str) -> bool {
        let now = Instant::now();
        let mut entries = self.entries.lock();
        entries.retain(|_, expires| *expires > now);
        entries.get(sid).is_some_and(|expires| *expires > now)
    }

    fn revoke(&self, sid: &str) {
        self.entries.lock().remove(sid);
    }

    pub(crate) fn login_is_allowed(&self, client_ip: Option<IpAddr>) -> bool {
        let now = Instant::now();
        let mut failures = self.auth_failures.lock();
        failures.retain(|_, entry| {
            entry.banned_until.is_some_and(|until| until > now)
                || now.duration_since(entry.last_failure) <= AUTH_FAILURE_WINDOW
        });
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
        if failures.len() >= MAX_TRACKED_CLIENTS
            && !failures.contains_key(&client_ip)
            && let Some(oldest) = failures
                .iter()
                .min_by_key(|(_, entry)| entry.last_failure)
                .map(|(ip, _)| *ip)
        {
            failures.remove(&oldest);
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

    if !crate::http_api::constant_time_credentials_match(
        expected_user,
        expected_pass,
        &login.username,
        &login.password,
    ) {
        state.qbittorrent_sessions.record_login_failure(client_ip);
        // Web API 2.8.1 predates the 2.14 change to a 401 response.
        return (StatusCode::OK, "Fails.").into_response();
    }

    state.qbittorrent_sessions.clear_login_failures(client_ip);
    let sid = state.qbittorrent_sessions.issue();
    let mut response = (StatusCode::OK, "Ok.").into_response();
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&format!(
            "SID={sid}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
            SESSION_TTL.as_secs()
        ))
        .expect("UUID SID always makes a valid cookie"),
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(super) async fn logout(State(state): State<ApiState>, headers: HeaderMap) -> Response {
    if let Some(sid) = sid_from_headers(&headers) {
        state.qbittorrent_sessions.revoke(sid);
    }
    let mut response = (StatusCode::OK, "Ok.").into_response();
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
        return next.run(request).await;
    }

    if sid_from_headers(&headers).is_some_and(|sid| state.qbittorrent_sessions.is_valid(sid)) {
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
    if headers.contains_key(AUTHORIZATION) {
        state.qbittorrent_sessions.record_login_failure(client_ip);
    }

    (StatusCode::FORBIDDEN, "Forbidden").into_response()
}

pub(super) async fn require_same_origin(
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
    let source = headers.get(ORIGIN).or_else(|| headers.get(REFERER));
    let Some(source) = source.and_then(|value| value.to_str().ok()) else {
        return true;
    };
    let Some(host) = headers.get(HOST).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    source
        .parse::<http::Uri>()
        .ok()
        .and_then(|uri| {
            uri.authority()
                .map(|authority| authority.as_str().to_owned())
        })
        .is_some_and(|authority| authority.eq_ignore_ascii_case(host))
}

fn sid_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| cookie.strip_prefix("SID="))
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use http::{HeaderMap, HeaderValue};

    use super::{MAX_AUTH_FAILURES, Sessions, request_source_matches_host};

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
    }

    #[test]
    fn revoked_session_is_invalid() {
        let sessions = Sessions::default();
        let sid = sessions.issue();
        assert!(sessions.is_valid(&sid));

        sessions.revoke(&sid);
        assert!(!sessions.is_valid(&sid));
    }
}
