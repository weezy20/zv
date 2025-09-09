use reqwest::Url;

#[derive(Debug, Clone)]
pub struct ZvNetwork {
    pub client: reqwest::Client,
}

impl Default for ZvNetwork {
    fn default() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("zv-cli/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("Failed to build HTTP client");

        Self { client }
    }
}
