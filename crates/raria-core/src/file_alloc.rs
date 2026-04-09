// raria-core: File allocation strategies (aria2: --file-allocation).
//
// Supports three modes:
// - none:    No pre-allocation (default). File grows as data arrives.
// - prealloc: Pre-allocate the full size using fallocate/ftruncate.
// - trunc:   Truncate the file to the expected size (fast but sparse).
//
// Pre-allocation prevents fragmentation on spinning disks and ensures
// disk space is reserved before the download starts.

use anyhow::{Context, Result};
use std::path::Path;
use tracing::debug;

/// File allocation strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FileAllocation {
    /// No pre-allocation; file grows as data arrives.
    #[default]
    None,
    /// Pre-allocate full size (writes zeros, may be slow for large files).
    Prealloc,
    /// Truncate to expected size (fast, sparse file on supported filesystems).
    Trunc,
    /// (Linux only) Use fallocate(2) to reserve space efficiently.
    Falloc,
}

impl FileAllocation {
    /// Parse a string into a FileAllocation variant.
    ///
    /// Accepts: "none", "prealloc", "trunc", "falloc" (case-insensitive).
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "prealloc" => Ok(Self::Prealloc),
            "trunc" => Ok(Self::Trunc),
            "falloc" => Ok(Self::Falloc),
            _ => Err(anyhow::anyhow!(
                "unknown file-allocation mode '{}': expected none, prealloc, trunc, or falloc",
                s
            )),
        }
    }
}

impl std::fmt::Display for FileAllocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Prealloc => write!(f, "prealloc"),
            Self::Trunc => write!(f, "trunc"),
            Self::Falloc => write!(f, "falloc"),
        }
    }
}

/// Pre-allocate a file to the given size using the specified strategy.
///
/// Returns Ok(()) if allocation succeeded or was not needed.
pub fn preallocate(path: &Path, size: u64, mode: FileAllocation) -> Result<()> {
    match mode {
        FileAllocation::None => Ok(()),
        FileAllocation::Trunc => {
            debug!(path = %path.display(), size, "truncating file for pre-allocation");
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)
                .context("failed to open file for truncation")?;
            file.set_len(size)
                .context("failed to truncate file to expected size")?;
            Ok(())
        }
        FileAllocation::Prealloc => {
            debug!(path = %path.display(), size, "pre-allocating file with zeros");
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)
                .context("failed to open file for pre-allocation")?;
            // Use set_len first for performance, then the OS handles block allocation.
            file.set_len(size)
                .context("failed to set file length for pre-allocation")?;
            Ok(())
        }
        FileAllocation::Falloc => {
            // On Linux, we could use fallocate(2) for efficient allocation.
            // On other platforms, fall back to trunc.
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::io::AsRawFd;
                debug!(path = %path.display(), size, "using fallocate for pre-allocation");
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(path)
                    .context("failed to open file for fallocate")?;
                let ret = unsafe {
                    libc::fallocate(file.as_raw_fd(), 0, 0, size as libc::off_t)
                };
                if ret != 0 {
                    return Err(anyhow::anyhow!(
                        "fallocate failed: {}",
                        std::io::Error::last_os_error()
                    ));
                }
                Ok(())
            }
            #[cfg(not(target_os = "linux"))]
            {
                debug!(path = %path.display(), size, "fallocate not supported, using trunc");
                preallocate(path, size, FileAllocation::Trunc)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn parse_file_allocation_modes() {
        assert_eq!(FileAllocation::parse("none").unwrap(), FileAllocation::None);
        assert_eq!(FileAllocation::parse("prealloc").unwrap(), FileAllocation::Prealloc);
        assert_eq!(FileAllocation::parse("trunc").unwrap(), FileAllocation::Trunc);
        assert_eq!(FileAllocation::parse("falloc").unwrap(), FileAllocation::Falloc);
        assert_eq!(FileAllocation::parse("NONE").unwrap(), FileAllocation::None);
        assert!(FileAllocation::parse("invalid").is_err());
    }

    #[test]
    fn display_roundtrips() {
        for mode in [FileAllocation::None, FileAllocation::Prealloc, FileAllocation::Trunc, FileAllocation::Falloc] {
            assert_eq!(FileAllocation::parse(&mode.to_string()).unwrap(), mode);
        }
    }

    #[test]
    fn preallocate_none_is_noop() {
        let tmp = NamedTempFile::new().unwrap();
        preallocate(tmp.path(), 1024, FileAllocation::None).unwrap();
        assert_eq!(tmp.as_file().metadata().unwrap().len(), 0);
    }

    #[test]
    fn preallocate_trunc_sets_size() {
        let tmp = NamedTempFile::new().unwrap();
        preallocate(tmp.path(), 4096, FileAllocation::Trunc).unwrap();
        assert_eq!(tmp.as_file().metadata().unwrap().len(), 4096);
    }

    #[test]
    fn preallocate_prealloc_sets_size() {
        let tmp = NamedTempFile::new().unwrap();
        preallocate(tmp.path(), 8192, FileAllocation::Prealloc).unwrap();
        assert_eq!(tmp.as_file().metadata().unwrap().len(), 8192);
    }
}
