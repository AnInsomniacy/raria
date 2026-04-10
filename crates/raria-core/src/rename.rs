use std::path::{Path, PathBuf};

/// Return a non-conflicting output path by appending `.N` to the full file name.
///
/// Examples:
/// - `file.zip`   -> `file.zip.1`
/// - `file.zip.1` -> `file.zip.2`
pub fn auto_rename(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");

    for suffix in 1..=u32::MAX {
        let candidate = parent.join(format!("{name}.{suffix}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::auto_rename;

    #[test]
    fn returns_original_path_when_file_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.bin");
        assert_eq!(auto_rename(&path), path);
    }

    #[test]
    fn appends_numeric_suffix_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.bin");
        std::fs::write(&path, b"existing").unwrap();

        assert_eq!(auto_rename(&path), dir.path().join("file.bin.1"));
    }

    #[test]
    fn skips_used_suffixes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.bin");
        std::fs::write(&path, b"existing").unwrap();
        std::fs::write(dir.path().join("file.bin.1"), b"existing").unwrap();

        assert_eq!(auto_rename(&path), dir.path().join("file.bin.2"));
    }
}
