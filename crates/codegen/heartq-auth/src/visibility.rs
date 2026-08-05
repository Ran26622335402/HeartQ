/// Apply auth headers to outbound visibility requests.
/// Implemented by `heartq-shell::util::heartq_auth_credentials::HeartQAuthCredentials`
/// to keep credential construction owned by shell while letting data-collector
/// build the request without reaching back into shell types.
pub trait HttpAuth: Send + Sync {
    fn apply(&self, builder: reqwest::RequestBuilder, base_url: &str) -> reqwest::RequestBuilder;
}
