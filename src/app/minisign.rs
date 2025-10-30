use std::io::Read;

use crate::ZvError;
use crate::app::constants::ZIG_MINSIGN_PUBKEY;
use color_eyre::Result;
use color_eyre::eyre::eyre;
use minisign_verify::{PublicKey, Signature};

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

    if !trusted_comment.contains(expected_filename) {
        return Err(ZvError::MinisignError(eyre!(
            "Signature filename mismatch: expected '{}' in trusted comment, got '{}'",
            expected_filename,
            trusted_comment
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
