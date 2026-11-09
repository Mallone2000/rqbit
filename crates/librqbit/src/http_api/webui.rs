use axum::{
    Router,
    extract::{OriginalUri, Request},
    routing::get,
};
use http::{HeaderMap, Method, header};

pub(super) fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/html"))
}

pub(super) fn is_external_landing_navigation(request: &Request) -> bool {
    let path = request
        .extensions()
        .get::<OriginalUri>()
        .map_or(request.uri(), |original| &original.0)
        .path();
    let headers = request.headers();
    let values = |name| headers.get_all(name).iter().map(|value| value.as_bytes());
    let mut sites = values("sec-fetch-site");

    // An external link may reach both redirects and the static login shell.
    // Use the original path because Axum strips /web/ inside the nested router.
    request.method() == Method::GET
        && (matches!(path, "/web" | "/web/") || (path == "/" && accepts_html(headers)))
        && !headers.contains_key(header::ORIGIN)
        && matches!(sites.next(), Some(b"cross-site" | b"same-site"))
        && sites.next().is_none()
        && values("sec-fetch-mode").eq([b"navigate".as_slice()])
        && values("sec-fetch-dest").eq([b"document".as_slice()])
}

pub fn make_webui_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/",
            get(|| async {
                (
                    [("Content-Type", "text/html")],
                    include_str!("../../webui/dist/index.html"),
                )
            }),
        )
        .route(
            "/assets/index.js",
            get(|| async {
                (
                    [("Content-Type", "application/javascript")],
                    include_str!("../../webui/dist/assets/index.js"),
                )
            }),
        )
        .route(
            "/assets/index.css",
            get(|| async {
                (
                    [("Content-Type", "text/css")],
                    include_str!("../../webui/dist/assets/index.css"),
                )
            }),
        )
        .route(
            "/assets/logo.svg",
            get(|| async {
                (
                    [("Content-Type", "image/svg+xml")],
                    include_str!("../../webui/dist/assets/logo.svg"),
                )
            }),
        )
}
