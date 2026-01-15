use std::io::Read;

use crate::ZvError;
use crate::app::constants::ZIG_MINSIGN_PUBKEY;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use minisign_verify::{PublicKey, Signature};

/// Extract filename from minisign trusted comment
/// Trusted comment format: "timestamp:<ts>\tfile:<filename>\t<metadata>"
fn extract_filename_from_trusted_comment(trusted_comment: &str) -> Result<String, ZvError> {
    for part in trusted_comment.split('\t') {
        if let Some(filename) = part.strip_prefix("file:") {
            return Ok(filename.to_string());
        }
    }

    Err(ZvError::MinisignError(eyre!(
        "Trusted comment missing 'file:' field: {}",
        trusted_comment
    )))
}

pub fn verify_minisign_signature(
    expected_filename: &str,
    tarball: &std::path::Path,
    signature: &std::path::Path,
) -> Result<(), ZvError> {
    let pubkey = PublicKey::from_base64(ZIG_MINSIGN_PUBKEY).map_err(|e| {
        ZvError::MinisignError(eyre!("Failed to parse public key from base64: {e}"))
    })?;
    let sig = Signature::from_file(signature)
        .map_err(|e| ZvError::MinisignError(eyre!("Failed to read signature file: {e}")))?;

    let trusted_comment = sig.trusted_comment();
    let actual_filename = extract_filename_from_trusted_comment(trusted_comment)?;

    if actual_filename != expected_filename {
        return Err(ZvError::MinisignError(eyre!(
            "Signature filename mismatch: expected '{}', got '{}'",
            expected_filename,
            actual_filename
        )));
    }

    // Stream verifier
    let mut verifier = pubkey
        .verify_stream(&sig)
        .map_err(|err| ZvError::MinisignError(eyre!("Failed to create stream verifier: {err}")))?;

    let mut file = std::fs::File::open(tarball)
        .map_err(|e| ZvError::MinisignError(eyre!("Failed to open tarball file: {e}")))?;
    let mut buf = [0u8; 8192];
    loop {
        let bytes_read = file
            .read(&mut buf)
            .map_err(|e| ZvError::MinisignError(eyre!("Failed to read tarball file: {e}")))?;
        if bytes_read == 0 {
            break; // End of file
        }

        verifier.update(&buf[..bytes_read]);
    }

    // Verify the signature
    verifier
        .finalize()
        .map_err(|e| ZvError::MinisignError(eyre!("Signature verification failed: {e}")))?;
    Ok(())
}
