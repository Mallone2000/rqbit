mod e2e;
mod e2e_another_local_client;
mod e2e_stream;
pub mod test_util;

#[test]
fn logged_urls_do_not_expose_credentials_paths_or_queries() {
    let redacted = crate::redact_url_for_logging(
        "https://private-user:private-pass@example.com/secret?token=private-token",
    );
    assert_eq!(redacted, "https://example.com");
    assert!(!redacted.contains("private"));
}
