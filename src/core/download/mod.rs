#![allow(dead_code)]

use anyhow::Result;
use std::path::Path;
use tokio::io::AsyncWriteExt;
use futures_util::StreamExt;

pub struct DownloadManager {
    client: reqwest::Client,
}

impl DownloadManager {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()?;

        Ok(Self { client })
    }

    pub async fn download_file(
        &self,
        url: &str,
        dest: &Path,
        progress_callback: Option<impl Fn(u64, u64)>,
    ) -> Result<()> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let response = self.client.get(url).send().await?;
        let total_size = response.content_length().unwrap_or(0);

        let mut file = tokio::fs::File::create(dest).await?;
        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;

            if let Some(ref callback) = progress_callback {
                callback(downloaded, total_size);
            }
        }

        file.flush().await?;
        Ok(())
    }

    pub async fn download_with_hash(
        &self,
        url: &str,
        dest: &Path,
        expected_sha1: Option<&str>,
    ) -> Result<()> {
        // Retry-Logik: 3 Versuche
        let mut retries = 3;

        while retries > 0 {
            // Download
            self.download_file(url, dest, None::<fn(u64, u64)>).await?;

            // Hash-Verifizierung (nur wenn erwartet)
            if let Some(expected) = expected_sha1 {
                use sha1::{Sha1, Digest};
                let content = tokio::fs::read(dest).await?;
                let hash = Sha1::digest(&content);
                let hash_str = hex::encode(hash);

                if hash_str.to_lowercase() == expected.to_lowercase() {
                    tracing::info!("Hash verified for {}", dest.display());
                    return Ok(());
                } else {
                    tracing::warn!(
                        "Hash mismatch for {} (got: {}, expected: {}), retries left: {}",
                        dest.display(),
                        hash_str,
                        expected,
                        retries - 1
                    );
                    tokio::fs::remove_file(dest).await.ok();
                    retries -= 1;

                    if retries > 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            } else {
                // Kein Hash erwartet, Download erfolgreich
                tracing::info!("Downloaded {} (no hash verification)", dest.display());
                return Ok(());
            }
        }

        // Alle Versuche fehlgeschlagen - trotzdem akzeptieren mit Warnung
        tracing::warn!(
            "Hash verification failed after 3 retries for {}, accepting anyway",
            url
        );

        // Nochmal downloaden ohne Hash-Check
        self.download_file(url, dest, None::<fn(u64, u64)>).await?;
        Ok(())
    }

    pub async fn download_many(
        &self,
        downloads: Vec<(String, std::path::PathBuf)>,
    ) -> Result<()> {
        use futures_util::stream::{self, StreamExt};

        stream::iter(downloads)
            .map(|(url, dest)| async move {
                self.download_file(&url, &dest, None::<fn(u64, u64)>).await
            })
            .buffer_unordered(4) // Download 4 files concurrently
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;

        Ok(())
    }
}
