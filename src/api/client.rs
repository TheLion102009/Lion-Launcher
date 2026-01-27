#![allow(dead_code)]

use anyhow::Result;
use reqwest::{Client, Response};
use std::time::Duration;

pub struct ApiClient {
    client: Client,
}

impl ApiClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(format!(
                "LionLauncher/{} ({})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS
            ))
            .build()?;

        Ok(Self { client })
    }

    pub async fn get(&self, url: &str) -> Result<Response> {
        let response = self.client.get(url).send().await?;
        Ok(response)
    }

    pub async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let response = self.get(url).await?;
        let data = response.json::<T>().await?;
        Ok(data)
    }

    pub async fn download_file(&self, url: &str) -> Result<bytes::Bytes> {
        let response = self.get(url).await?;
        let bytes = response.bytes().await?;
        Ok(bytes)
    }

    pub fn get_client(&self) -> &Client {
        &self.client
    }
}

impl Default for ApiClient {
    fn default() -> Self {
        Self::new().expect("Failed to create API client")
    }
}
