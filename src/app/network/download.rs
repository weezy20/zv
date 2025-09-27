use std::{
    path::Path,
    time::{Duration, Instant},
};

use color_eyre::eyre::Context;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::{NetErr, ZvError, app::utils::ProgressHandle};

const TARGET: &str = "zv::network::download";

/// Download a single file with HTTP status code handling (standalone version)
///
/// This function handles the complete download process for a single file with comprehensive
/// error handling and logging for different failure scenarios.
pub(in crate::app::network) async fn download_file_with_retries_standalone(
    client: &reqwest::Client,
    url: &str,
    dest_path: &Path,
    expected_size: u64,
    progress_handle: &ProgressHandle,
) -> Result<(), NetErr> {
    tracing::debug!(target: TARGET, "Starting download request for URL: {}", url);

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                tracing::warn!(target: TARGET, "Request timeout for URL: {} - This may indicate network connectivity issues or server overload", url);
                NetErr::Timeout(format!("Request timeout for {}", url))
            } else if e.is_connect() {
                tracing::warn!(target: TARGET, "Connection error for URL: {} - Unable to establish connection to server", url);
                NetErr::Reqwest(e)
            } else {
                tracing::error!(target: TARGET, "Network error during request to {}: {} - This may indicate DNS issues or network problems", url, e);
                NetErr::Reqwest(e)
            }
        })?;

    let status = response.status();
    tracing::debug!(target: TARGET, "Received HTTP response with status: {} for URL: {}", status, url);

    // Handle specific HTTP status codes as retriable failures with detailed explanations
    match status.as_u16() {
        200 => {
            // Success, proceed with download
            tracing::trace!(target: TARGET, "HTTP 200 OK received, proceeding with file download from {}", url);
        }
        503 => {
            tracing::warn!(target: TARGET, "HTTP 503 Service Unavailable for URL: {} - Mirror is experiencing scheduled downtime or maintenance. Will retry with different mirror.", url);
            return Err(NetErr::HTTP(status));
        }
        429 => {
            tracing::warn!(target: TARGET, "HTTP 429 Too Many Requests for URL: {} - Mirror is rate limiting requests. Will retry with different mirror after delay.", url);
            return Err(NetErr::HTTP(status));
        }
        404 => {
            tracing::warn!(target: TARGET, "HTTP 404 Not Found for URL: {} - File may not exist on this mirror (common for old Zig versions ≤0.5.0). Will retry with different mirror.", url);
            return Err(NetErr::HTTP(status));
        }
        504 => {
            tracing::warn!(target: TARGET, "HTTP 504 Gateway Timeout for URL: {} - Mirror gateway is experiencing issues or ziglang.org is inaccessible. Will retry with different mirror.", url);
            return Err(NetErr::HTTP(status));
        }
        500..=599 => {
            tracing::warn!(target: TARGET, "HTTP {} Server Error for URL: {} - Mirror is experiencing server-side issues. Will retry with different mirror.", status, url);
            return Err(NetErr::HTTP(status));
        }
        400..=499 => {
            tracing::warn!(target: TARGET, "HTTP {} Client Error for URL: {} - Request may be malformed or unauthorized. Will retry with different mirror.", status, url);
            return Err(NetErr::HTTP(status));
        }
        _ => {
            tracing::warn!(target: TARGET, "Unexpected HTTP status {} for URL: {} - Unknown response code. Will retry with different mirror.", status, url);
            return Err(NetErr::HTTP(status));
        }
    }
    tracing::trace!(target: TARGET, "Initiating streaming download for {} bytes from {}", expected_size, url);
    match stream_download_file(client, url, dest_path, expected_size, progress_handle).await {
        Ok(()) => {
            tracing::debug!(target: TARGET, "Successfully completed download from {}", url);
            Ok(())
        }
        Err(e) => {
            tracing::error!(target: TARGET, "Download failed from {}: {}", url, e);
            Err(e)
        }
    }
}

/// Stream download a file from URL to destination path with progress reporting
///
/// Downloads a file using reqwest streaming API, writing data to the specified destination
/// path while reporting progress through the provided ProgressHandle. The function uses
/// the content-length header to calculate download progress percentage.
///
/// # Arguments
/// * `client` - HTTP client to use for the request
/// * `url` - URL to download from
/// * `dest_path` - Destination file path to write to
/// * `expected_size` - Expected file size in bytes for progress calculation
/// * `progress_handle` - Handle for progress reporting
///
/// # Returns
/// * `Ok(())` on successful download
/// * `Err(NetErr)` on network errors, timeouts, or file I/O errors
pub(in crate::app::network) async fn stream_download_file(
    client: &reqwest::Client,
    url: &str,
    dest_path: &Path,
    expected_size: u64,
    progress_handle: &ProgressHandle,
) -> Result<(), NetErr> {
    // Start the download request
    let response = client.get(url).send().await.map_err(|e| {
        if e.is_timeout() {
            tracing::warn!(target: TARGET, "Download timeout for URL: {}", url);
            NetErr::Timeout(format!("Request timeout for {}", url))
        } else if e.is_connect() {
            tracing::warn!(target: TARGET, "Connection error for URL: {}", url);
            NetErr::Reqwest(e)
        } else {
            tracing::error!(target: TARGET, "Network error during download: {}", e);
            NetErr::Reqwest(e)
        }
    })?;

    // Check response status
    if !response.status().is_success() {
        let status = response.status();
        tracing::error!(target: TARGET, "HTTP error {} for URL: {}", status, url);
        return Err(NetErr::HTTP(status));
    }

    // Get content length for progress calculation
    let content_length = response.content_length().unwrap_or(expected_size);
    tracing::debug!(target: TARGET, "Starting download: {} bytes from {}", content_length, url);

    // Create the destination file
    let mut file = tokio::fs::File::create(dest_path)
        .await
        .map_err(ZvError::Io)
        .wrap_err_with(|| format!("Failed to create destination file: {}", dest_path.display()))?;

    // Stream the response body
    let mut stream = response.bytes_stream();
    let mut downloaded = 0u64;
    let mut last_progress_update = Instant::now();
    const PROGRESS_UPDATE_INTERVAL: Duration = Duration::from_millis(250); // Update progress every 250ms

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| {
            tracing::error!(target: TARGET, "Error reading chunk during download: {}", e);
            NetErr::Reqwest(e)
        })?;

        // Write chunk to file
        file.write_all(&chunk)
            .await
            .map_err(ZvError::Io)
            .wrap_err_with(|| {
                format!(
                    "Failed to write to destination file: {}",
                    dest_path.display()
                )
            })?;

        downloaded += chunk.len() as u64;

        // Update progress periodically to avoid overwhelming the progress bar
        let now = Instant::now();
        if now.duration_since(last_progress_update) >= PROGRESS_UPDATE_INTERVAL {
            let percentage = if content_length > 0 {
                (downloaded * 100) / content_length
            } else {
                0
            };

            let downloaded_mb = downloaded as f64 / 1_048_576.0; // Convert to MB
            let total_mb = content_length as f64 / 1_048_576.0;

            let progress_msg = if content_length > 0 {
                format!(
                    "Downloading {:.1}/{:.1} MB ({}%)",
                    downloaded_mb, total_mb, percentage
                )
            } else {
                format!("Downloading {:.1} MB", downloaded_mb)
            };

            if let Err(e) = progress_handle.update(progress_msg).await {
                tracing::warn!(target: TARGET, "Failed to update progress: {}", e);
            }

            last_progress_update = now;
        }
    }

    // Ensure all data is written to disk
    file.flush()
        .await
        .map_err(ZvError::Io)
        .wrap_err_with(|| format!("Failed to flush file: {}", dest_path.display()))?;

    // Final progress update
    let downloaded_mb = downloaded as f64 / 1_048_576.0;
    let final_msg = format!("Download completed: {:.1} MB", downloaded_mb);
    if let Err(e) = progress_handle.update(final_msg).await {
        tracing::warn!(target: TARGET, "Failed to update final progress: {}", e);
    }

    tracing::trace!(target: TARGET, "Successfully downloaded {} bytes to {}", downloaded, dest_path.display());
    Ok(())
}

/// Move file from temporary location to final destination atomically
///
/// Performs an atomic move operation from a temporary file path to the final destination.
/// This ensures that the file only appears in the final location after it has been
/// completely written and verified. Enhanced with comprehensive error handling and logging.
///
/// # Arguments
/// * `temp_path` - Path to the temporary file to move
/// * `final_path` - Destination path for the final file
///
/// # Returns
/// * `Ok(())` on successful move
/// * `Err(std::io::Error)` on filesystem errors during the move operation
pub(in crate::app::network) async fn move_to_final_location(
    temp_path: &Path,
    final_path: &Path,
) -> Result<(), std::io::Error> {
    tracing::debug!(target: TARGET, "Starting atomic move from {} to {}", temp_path.display(), final_path.display());

    // Validate input paths
    if !temp_path.exists() {
        let error_msg = format!("Source file does not exist: {}", temp_path.display());
        tracing::error!(target: TARGET, "{}", error_msg);
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, error_msg));
    }

    // Get file size for logging
    let file_size = match tokio::fs::metadata(temp_path).await {
        Ok(metadata) => {
            let size = metadata.len();
            tracing::debug!(target: TARGET, "Source file size: {} bytes ({:.1} MB)", size, size as f64 / 1_048_576.0);
            Some(size)
        }
        Err(e) => {
            tracing::warn!(target: TARGET, "Could not read source file metadata: {} - proceeding with move", e);
            None
        }
    };

    // Ensure the parent directory of the final path exists
    if let Some(parent) = final_path.parent() {
        if !parent.exists() {
            tracing::debug!(target: TARGET, "Creating parent directory: {}", parent.display());
            match tokio::fs::create_dir_all(parent).await {
                Ok(()) => {
                    tracing::debug!(target: TARGET, "Successfully created parent directory: {}", parent.display());
                }
                Err(e) => {
                    tracing::error!(target: TARGET, "Failed to create parent directory {}: {}", parent.display(), e);

                    // Provide specific error context
                    match e.kind() {
                        std::io::ErrorKind::PermissionDenied => {
                            tracing::error!(target: TARGET, "Permission denied creating directory {} - check write permissions", parent.display());
                        }
                        std::io::ErrorKind::AlreadyExists => {
                            tracing::debug!(target: TARGET, "Parent directory {} already exists - this is normal", parent.display());
                        }
                        _ => {
                            tracing::error!(target: TARGET, "Unexpected error creating directory {}: {:?}", parent.display(), e.kind());
                        }
                    }
                    return Err(e);
                }
            }
        } else {
            tracing::debug!(target: TARGET, "Parent directory already exists: {}", parent.display());
        }
    }

    // Check if destination already exists and handle appropriately
    if final_path.exists() {
        tracing::warn!(target: TARGET, "Destination file already exists: {} - will overwrite", final_path.display());

        // Get existing file size for comparison
        if let Ok(existing_metadata) = tokio::fs::metadata(final_path).await {
            let existing_size = existing_metadata.len();
            tracing::debug!(target: TARGET, "Existing file size: {} bytes ({:.1} MB)", existing_size, existing_size as f64 / 1_048_576.0);

            if let Some(new_size) = file_size {
                if existing_size == new_size {
                    tracing::debug!(target: TARGET, "File sizes match - this may be a duplicate download");
                } else {
                    tracing::debug!(target: TARGET, "File sizes differ - replacing with new version");
                }
            }
        }
    }

    // Perform the atomic move
    tracing::debug!(target: TARGET, "Performing atomic file move operation");
    match tokio::fs::rename(temp_path, final_path).await {
        Ok(()) => {
            tracing::trace!(target: TARGET, "Successfully moved file from {} to {} ({})",
            temp_path.display(), final_path.display(),
            if let Some(size) = file_size {
                format!("{:.1} MB", size as f64 / 1_048_576.0)
            } else {
                "unknown size".to_string()
            });

            // Verify the move was successful
            if final_path.exists() && !temp_path.exists() {
                tracing::debug!(target: TARGET, "Atomic move verification successful - file exists at destination and removed from source");
            } else {
                tracing::warn!(target: TARGET, "Atomic move verification failed - file state may be inconsistent");
            }

            Ok(())
        }
        Err(e) => {
            tracing::error!(target: TARGET, "Failed to move file from {} to {}: {}", temp_path.display(), final_path.display(), e);

            // Provide specific error context for troubleshooting
            match e.kind() {
                std::io::ErrorKind::PermissionDenied => {
                    tracing::error!(target: TARGET, "Permission denied during file move - check write permissions for destination directory");
                }
                std::io::ErrorKind::NotFound => {
                    tracing::error!(target: TARGET, "Source file disappeared during move operation - this indicates a race condition or external interference");
                }
                std::io::ErrorKind::AlreadyExists => {
                    tracing::error!(target: TARGET, "Destination file exists and cannot be overwritten - this may indicate a filesystem issue");
                }
                std::io::ErrorKind::InvalidInput => {
                    tracing::error!(target: TARGET, "Invalid file paths provided for move operation");
                }
                _ => {
                    tracing::error!(target: TARGET, "Unexpected I/O error during file move: {:?}", e.kind());
                }
            }

            Err(e)
        }
    }
}
