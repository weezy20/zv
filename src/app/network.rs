use color_eyre::eyre::{Result, WrapErr, eyre};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Url;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use yansi::Paint;

use crate::{NetErr, ZigVersion, ZvError};

#[derive(Debug, Clone)]
pub struct ZvNetwork {
    pub client: reqwest::Client,
}

impl Default for ZvNetwork {
    fn default() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("zv-cli/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30)) // 30 second timeout
            .build()
            .expect("Failed to build HTTP client");

        Self { client }
    }
}

impl ZvNetwork {
    /// Download a file with progress reporting
    pub async fn download_file<U>(&self, url: U, destination: &PathBuf) -> Result<(), NetErr>
    where
        U: AsRef<str> + std::fmt::Display + std::fmt::Debug,
    {
        // Extract filename from URL for display
        let filename = url.as_ref()
            .split('/')
            .last()
            .unwrap_or("file");

        // Try to get content length for progress bar
        let content_length = self.get_content_length(&url).await.ok();

        // Create progress bar
        let pb = if let Some(total) = content_length {
            let pb = ProgressBar::new(total);
            pb.set_style(ProgressStyle::default_bar()
                .template("{msg} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("##-"));
            pb.set_message(format!("Downloading {}", filename));
            pb
        } else {
            let pb = ProgressBar::new_spinner();
            pb.set_style(ProgressStyle::default_spinner()
                .template("{msg} {spinner:.green}")
                .unwrap());
            pb.set_message(format!("Downloading {}", filename));
            pb
        };

        // Perform the download
        let res = self
            .client
            .get(url.as_ref())
            .send()
            .await
            .map_err(|e| eyre!("Failed to send request: {}", e))?;

        let mut file = tokio::fs::File::create(destination)
            .await
            .map_err(NetErr::FileIo)
            .context("Failed to create file")?;

        let mut stream = res.bytes_stream();
        let mut downloaded: u64 = 0;

        while let Some(item) = stream.next().await {
            let chunk = item
                .map_err(NetErr::Network)
                .context("Failed to download chunk")?;

            file.write_all(&chunk)
                .await
                .map_err(NetErr::FileIo)
                .context("Failed to write chunk to file")?;

            downloaded += chunk.len() as u64;
            pb.set_position(downloaded);
        }

        pb.finish_and_clear();
        println!("✓ Downloaded {}", Paint::green(filename));
        
        // Keep debug info for when ZV_LOG is enabled
        tracing::debug!("Download completed: {} -> {}", url, destination.display());
        Ok(())
    }

    /// Get the content length of a remote file
    async fn get_content_length<U>(&self, url: U) -> Result<u64>
    where
        U: AsRef<str> + std::fmt::Display + std::fmt::Debug,
    {
        let res = self
            .client
            .head(url.as_ref())
            .send()
            .await
            .map_err(|e| eyre!("Failed to send HEAD request to {}: {}", url, e))?;

        if !res.status().is_success() {
            return Err(eyre!(
                "HEAD request to {} returned status: {}",
                url,
                res.status()
            ));
        }

        let length = res
            .headers()
            .get("content-length")
            .and_then(|ct_len| ct_len.to_str().ok())
            .and_then(|ct_len| ct_len.parse().ok())
            .ok_or_else(|| {
                eyre!(
                    "No valid content-length header found in response from {}",
                    url
                )
            })?;

        Ok(length)
    }
}

#[cfg(test)]
mod tests {
    use crate::{app::utils::zig_tarball, init_tracing};

    use super::*;
    use color_eyre::eyre::bail;
    use indicatif::{ProgressBar, ProgressStyle};
    use std::time::Duration;
    use tempfile::NamedTempFile;

    #[tokio::test]
    #[tracing::instrument]
    async fn download() -> color_eyre::Result<()> {
        color_eyre::install()?;
        init_tracing()?;
        let network = ZvNetwork::default();

        // Create a temporary file for the download
        let temp_file = NamedTempFile::new()?;
        let destination = temp_file.path().to_path_buf();
        let tarball = zig_tarball(
            &<ZigVersion as std::str::FromStr>::from_str("0.15.1").unwrap(),
            None,
        )
        .unwrap();
        let url = format!("https://ziglang.org/download/0.15.1/{}", tarball);

        tracing::info!("Starting download from: {}", url);

        // Record start time for performance measurement
        let start_time = std::time::Instant::now();

        // Download the file
        network.download_file(url.as_str(), &destination).await?;

        // Verify the download
        let downloaded_size = tokio::fs::metadata(&destination).await?.len();

        // Calculate and log download speed
        let elapsed = start_time.elapsed();
        let speed_mbps = (downloaded_size as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64();
        tracing::info!(
            "Download completed in {:.2}s at {:.2} MB/s",
            elapsed.as_secs_f64(),
            speed_mbps
        );

        // Clean up the temporary file
        temp_file.close()?;

        Ok(())
    }
}
