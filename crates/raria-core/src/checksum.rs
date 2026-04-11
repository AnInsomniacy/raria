// raria-core: Checksum verification.
//
// Computes digests of downloaded files for integrity verification.
// Supports SHA-256 (primary), SHA-1, and MD5 (legacy).

use crate::job::PieceChecksum;
use anyhow::{Context, Result};
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

/// Supported checksum algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumAlgo {
    /// SHA-256 (recommended, 256-bit digest).
    Sha256,
    /// SHA-1 (legacy, 160-bit digest).
    Sha1,
    /// MD5 (legacy, 128-bit digest).
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

/// Generic file hasher — works for any `Digest` implementation.
async fn hash_file<D: Digest>(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .await
        .with_context(|| format!("failed to open file for checksum: {}", path.display()))?;

    let mut hasher = D::new();
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

/// Compute the SHA-256 digest of a file, returning the hex string.
pub async fn sha256_file(path: &Path) -> Result<String> {
    hash_file::<Sha256>(path).await
}

/// Compute the SHA-1 digest of a file, returning the hex string.
pub async fn sha1_file(path: &Path) -> Result<String> {
    hash_file::<Sha1>(path).await
}

/// Compute the MD5 digest of a file, returning the hex string.
pub async fn md5_file(path: &Path) -> Result<String> {
    hash_file::<Md5>(path).await
}

/// Verify a file against a checksum specification.
///
/// Returns `Ok(())` if the checksum matches, or an error otherwise.
pub async fn verify_checksum(path: &Path, spec: &str) -> Result<()> {
    let (algo, expected) = parse_checksum_spec(spec)?;

    let actual = match algo {
        ChecksumAlgo::Sha256 => sha256_file(path).await?,
        ChecksumAlgo::Sha1 => sha1_file(path).await?,
        ChecksumAlgo::Md5 => md5_file(path).await?,
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

/// Verify a file against chunk-level piece hashes.
pub async fn verify_piece_checksums(path: &Path, piece_checksum: &PieceChecksum) -> Result<()> {
    anyhow::ensure!(
        piece_checksum.length > 0,
        "piece checksum length must be > 0"
    );
    anyhow::ensure!(
        !piece_checksum.hashes.is_empty(),
        "piece checksum list must not be empty"
    );

    let algo = ChecksumAlgo::from_str_lenient(&piece_checksum.algo).with_context(|| {
        format!(
            "unsupported piece checksum algorithm: {}",
            piece_checksum.algo
        )
    })?;

    let chunk_len: usize = piece_checksum
        .length
        .try_into()
        .context("piece checksum length does not fit into usize")?;
    let mut file = File::open(path)
        .await
        .with_context(|| format!("failed to open file for piece checksum: {}", path.display()))?;

    let mut buffer = vec![0u8; chunk_len];

    for (index, expected_hash) in piece_checksum.hashes.iter().enumerate() {
        let mut read_total = 0usize;
        while read_total < chunk_len {
            let n = file.read(&mut buffer[read_total..chunk_len]).await?;
            if n == 0 {
                break;
            }
            read_total += n;
        }

        anyhow::ensure!(
            read_total > 0,
            "piece checksum mismatch for {}: missing piece {}",
            path.display(),
            index
        );

        let actual = match algo {
            ChecksumAlgo::Sha256 => {
                let mut hasher = Sha256::new();
                hasher.update(&buffer[..read_total]);
                hex::encode(hasher.finalize())
            }
            ChecksumAlgo::Sha1 => {
                let mut hasher = Sha1::new();
                hasher.update(&buffer[..read_total]);
                hex::encode(hasher.finalize())
            }
            ChecksumAlgo::Md5 => {
                let mut hasher = Md5::new();
                hasher.update(&buffer[..read_total]);
                hex::encode(hasher.finalize())
            }
        };

        let expected = expected_hash.to_lowercase();
        if actual != expected {
            anyhow::bail!(
                "piece checksum mismatch for {}: piece {} expected {} {}, got {}",
                path.display(),
                index,
                algo.name(),
                expected,
                actual
            );
        }
    }

    let mut trailing = [0u8; 1];
    let trailing_n = file.read(&mut trailing).await?;
    anyhow::ensure!(
        trailing_n == 0,
        "piece checksum mismatch for {}: file contains more data than described pieces",
        path.display()
    );

    Ok(())
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

        let result = verify_checksum(
            &path,
            "sha-256=0000000000000000000000000000000000000000000000000000000000000000",
        )
        .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("checksum mismatch"), "error was: {err}");
    }

    #[tokio::test]
    async fn verify_checksum_file_not_found() {
        let result = verify_checksum(Path::new("/nonexistent/path/file.bin"), "sha-256=abc").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn verify_piece_checksums_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pieces.bin");
        std::fs::write(&path, b"abcdefgh").unwrap();

        let piece_checksum = PieceChecksum {
            algo: "sha-256".into(),
            length: 4,
            hashes: vec![
                hex::encode(Sha256::digest(b"abcd")),
                hex::encode(Sha256::digest(b"efgh")),
            ],
        };

        let result = verify_piece_checksums(&path, &piece_checksum).await;
        assert!(result.is_ok(), "piece checksums should match: {result:?}");
    }

    #[tokio::test]
    async fn verify_piece_checksums_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pieces.bin");
        std::fs::write(&path, b"abcdefgh").unwrap();

        let piece_checksum = PieceChecksum {
            algo: "sha-256".into(),
            length: 4,
            hashes: vec!["00".repeat(32), hex::encode(Sha256::digest(b"efgh"))],
        };

        let result = verify_piece_checksums(&path, &piece_checksum).await;
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("piece checksum mismatch"),
            "error was: {error}"
        );
    }

    #[tokio::test]
    async fn verify_piece_checksums_rejects_extra_file_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pieces.bin");
        std::fs::write(&path, b"abcdefgh").unwrap();

        let piece_checksum = PieceChecksum {
            algo: "sha-256".into(),
            length: 4,
            hashes: vec![hex::encode(Sha256::digest(b"abcd"))],
        };

        let result = verify_piece_checksums(&path, &piece_checksum).await;
        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("more data"), "error was: {error}");
    }

    #[test]
    fn algo_name_roundtrip() {
        assert_eq!(ChecksumAlgo::Sha256.name(), "sha-256");
        assert_eq!(ChecksumAlgo::Sha1.name(), "sha-1");
        assert_eq!(ChecksumAlgo::Md5.name(), "md5");
    }

    // ── SHA-1 tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn sha1_of_known_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sha1_test.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello world\n").unwrap();
        }

        let hash = sha1_file(&path).await.unwrap();
        // echo "hello world" | sha1sum = 22596363b3de40b06f981fb85d82312e8c0ed511
        assert_eq!(hash, "22596363b3de40b06f981fb85d82312e8c0ed511");
    }

    #[tokio::test]
    async fn sha1_of_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty_sha1.txt");
        std::fs::File::create(&path).unwrap();

        let hash = sha1_file(&path).await.unwrap();
        // sha1("") = da39a3ee5e6b4b0d3255bfef95601890afd80709
        assert_eq!(hash, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[tokio::test]
    async fn verify_sha1_checksum_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sha1_match.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello world\n").unwrap();
        }

        let result = verify_checksum(&path, "sha-1=22596363b3de40b06f981fb85d82312e8c0ed511").await;
        assert!(result.is_ok());
    }

    // ── MD5 tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn md5_of_known_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("md5_test.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello world\n").unwrap();
        }

        let hash = md5_file(&path).await.unwrap();
        // echo "hello world" | md5sum = 6f5902ac237024bdd0c176cb93063dc4
        assert_eq!(hash, "6f5902ac237024bdd0c176cb93063dc4");
    }

    #[tokio::test]
    async fn md5_of_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty_md5.txt");
        std::fs::File::create(&path).unwrap();

        let hash = md5_file(&path).await.unwrap();
        // md5("") = d41d8cd98f00b204e9800998ecf8427e
        assert_eq!(hash, "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[tokio::test]
    async fn verify_md5_checksum_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("md5_match.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello world\n").unwrap();
        }

        let result = verify_checksum(&path, "md5=6f5902ac237024bdd0c176cb93063dc4").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn verify_md5_checksum_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("md5_mismatch.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello world\n").unwrap();
        }

        let result = verify_checksum(&path, "md5=00000000000000000000000000000000").await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("checksum mismatch")
        );
    }
}
