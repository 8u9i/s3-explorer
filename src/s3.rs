use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use aws_credential_types::Credentials;
use aws_sdk_s3::config::{Builder, Region};
use aws_sdk_s3::Client;

#[derive(Clone, Debug)]
pub struct S3Config {
    pub bucket: String,
    pub endpoint: String,
    #[allow(dead_code)]
    pub region: String,
    pub presign_ttl: Duration,
    pub max_upload_bytes: usize,
}

impl S3Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bucket = require_env("BUCKET")?;
        let endpoint =
            std::env::var("ENDPOINT").unwrap_or_else(|_| "https://storage.railway.app".into());
        let region = std::env::var("REGION").unwrap_or_else(|_| "auto".into());

        let presign_ttl = std::env::var("PRESIGN_TTL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(900));

        let max_upload_bytes = std::env::var("MAX_UPLOAD_BYTES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(104_857_600);

        Ok(Self {
            bucket,
            endpoint,
            region,
            presign_ttl,
            max_upload_bytes,
        })
    }
}

#[derive(Clone)]
pub struct S3Ctx {
    pub client: Client,
    pub config: Arc<S3Config>,
}

pub async fn client_from_env_async() -> anyhow::Result<Client> {
    let key = require_env("ACCESS_KEY_ID")?;
    let secret = require_env("SECRET_ACCESS_KEY")?;
    let endpoint =
        std::env::var("ENDPOINT").unwrap_or_else(|_| "https://storage.railway.app".into());
    let region = std::env::var("REGION").unwrap_or_else(|_| "auto".into());

    let creds = Credentials::new(key, secret, None, None, "railway");

    let shared = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(Region::new(region.clone()))
        .endpoint_url(&endpoint)
        .credentials_provider(creds)
        .load()
        .await;

    let builder = Builder::from(&shared)
        .region(Region::new(region))
        .endpoint_url(&endpoint)
        .force_path_style(true);

    Ok(Client::from_conf(builder.build()))
}

pub async fn build_context() -> anyhow::Result<S3Ctx> {
    let config = S3Config::from_env().context("loading S3 config from env")?;
    let client = client_from_env_async()
        .await
        .context("building S3 client")?;
    Ok(S3Ctx {
        client,
        config: Arc::new(config),
    })
}

fn require_env(name: &str) -> anyhow::Result<String> {
    let v = std::env::var(name).map_err(|_| anyhow::anyhow!("required env var {name} is not set"))?;
    if v.trim().is_empty() {
        anyhow::bail!("required env var {name} is empty");
    }
    Ok(v)
}
