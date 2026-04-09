// raria-core: Checksum verification.
//
// Computes digests of downloaded files for integrity verification.
// Supports SHA-256 (primary), SHA-1, and MD5 (legacy).

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

/// Supported checksum algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumAlgo {
    Sha256,
    Sha1,
    Md5,
}

impl ChecksumAlgo {
    /// Parse from a string like "sha-256", "sha256", "sha-1", "md5".
    pub fn from_str_lenient(s: &str) -> Option<Self> {
        match s.to_lowercase().replace('-', "").as_str() {
            "sha256" => Some(Self::Sha256),
            "sha1" => Some(Self::Sha1),
            "md5" => Some(Self::Md5),
            _ => None,
        }
    }

    /// Name used in output.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Sha256 => "sha-256",
            Self::Sha1 => "sha-1",
            Self::Md5 => "md5",
        }
    }
}

/// Parse a checksum spec like "sha-256=abc123" into (algo, expected_hex).
pub fn parse_checksum_spec(spec: &str) -> Result<(ChecksumAlgo, String)> {
    let (algo_str, hex) = spec
        .split_once('=')
        .context("checksum spec must be in format 'algo=hex'")?;

    let algo = ChecksumAlgo::from_str_lenient(algo_str)
        .with_context(|| format!("unsupported checksum algorithm: {algo_str}"))?;

    let hex = hex.trim().to_lowercase();
    anyhow::ensure!(!hex.is_empty(), "checksum hex value is empty");

    Ok((algo, hex))
}

/// Compute the SHA-256 digest of a file, returning the hex string.
pub async fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .await
        .with_context(|| format!("failed to open file for checksum: {}", path.display()))?;

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];

    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// Verify a file against a checksum specification.
///
/// Returns `Ok(())` if the checksum matches, or an error otherwise.
pub async fn verify_checksum(path: &Path, spec: &str) -> Result<()> {
    let (algo, expected) = parse_checksum_spec(spec)?;

    let actual = match algo {
        ChecksumAlgo::Sha256 => sha256_file(path).await?,
        // sha-1 and md5 share the same pattern but use different hashers.
        // For now, only sha-256 is fully implemented.
        ChecksumAlgo::Sha1 | ChecksumAlgo::Md5 => {
            anyhow::bail!("{} is not yet implemented", algo.name());
        }
    };

    if actual == expected {
        Ok(())
    } else {
        anyhow::bail!(
            "checksum mismatch for {}: expected {} {}, got {}",
            path.display(),
            algo.name(),
            expected,
            actual
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_sha256_spec() {
        let (algo, hex) = parse_checksum_spec("sha-256=abcdef1234567890").unwrap();
        assert_eq!(algo, ChecksumAlgo::Sha256);
        assert_eq!(hex, "abcdef1234567890");
    }

    #[test]
    fn parse_sha256_spec_no_dash() {
        let (algo, _) = parse_checksum_spec("sha256=abc").unwrap();
        assert_eq!(algo, ChecksumAlgo::Sha256);
    }

    #[test]
    fn parse_md5_spec() {
        let (algo, _) = parse_checksum_spec("md5=abc123").unwrap();
        assert_eq!(algo, ChecksumAlgo::Md5);
    }

    #[test]
    fn parse_spec_trims_whitespace() {
        let (_, hex) = parse_checksum_spec("sha-256= AbCdEf ").unwrap();
        assert_eq!(hex, "abcdef");
    }

    #[test]
    fn parse_spec_missing_equals_fails() {
        assert!(parse_checksum_spec("sha256abcdef").is_err());
    }

    #[test]
    fn parse_spec_unknown_algo_fails() {
        assert!(parse_checksum_spec("blake3=abc").is_err());
    }

    #[test]
    fn parse_spec_empty_hex_fails() {
        assert!(parse_checksum_spec("sha-256=").is_err());
    }

    #[tokio::test]
    async fn sha256_of_known_content() {
        // SHA-256 of "hello world\n" (echo "hello world" | sha256sum)
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello world\n").unwrap();
        }

        let hash = sha256_file(&path).await.unwrap();
        // sha256("hello world\n") = a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447
        assert_eq!(
            hash,
            "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447"
        );
    }

    #[tokio::test]
    async fn sha256_of_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        std::fs::File::create(&path).unwrap();

        let hash = sha256_file(&path).await.unwrap();
        // sha256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[tokio::test]
    async fn verify_checksum_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("match.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello world\n").unwrap();
        }

        let result = verify_checksum(
            &path,
            "sha-256=a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447",
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn verify_checksum_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mismatch.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello world\n").unwrap();
        }

        let result = verify_checksum(&path, "sha-256=0000000000000000000000000000000000000000000000000000000000000000").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("checksum mismatch"), "error was: {err}");
    }

    #[tokio::test]
    async fn verify_checksum_file_not_found() {
        let result = verify_checksum(
            Path::new("/nonexistent/path/file.bin"),
            "sha-256=abc",
        )
        .await;
        assert!(result.is_err());
    }

    #[test]
    fn algo_name_roundtrip() {
        assert_eq!(ChecksumAlgo::Sha256.name(), "sha-256");
        assert_eq!(ChecksumAlgo::Sha1.name(), "sha-1");
        assert_eq!(ChecksumAlgo::Md5.name(), "md5");
    }
}
