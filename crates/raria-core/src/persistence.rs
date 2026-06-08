use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::{fs, io::AsyncWriteExt};

use crate::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlFile {
    pub format: String,
    pub version: u32,
    pub gid: String,
    pub kind: String,
    pub target: ControlTarget,
    pub pieces: ControlPieces,
    pub sources: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlTarget {
    pub dir: String,
    pub name: String,
    pub length: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlPieces {
    pub piece_length: u64,
    pub bitfield: String,
}

impl ControlFile {
    pub fn new_http(
        gid: impl Into<String>,
        dir: &Path,
        name: impl Into<String>,
        length: Option<u64>,
        completed_length: u64,
        sources: Vec<String>,
    ) -> Self {
        Self {
            format: "raria-control".into(),
            version: 1,
            gid: gid.into(),
            kind: "http".into(),
            target: ControlTarget {
                dir: dir.to_string_lossy().into_owned(),
                name: name.into(),
                length,
            },
            pieces: ControlPieces {
                piece_length: 1,
                bitfield: completed_length.to_string(),
            },
            sources,
        }
    }

    pub fn completed_length(&self) -> Result<u64> {
        self.pieces
            .bitfield
            .parse::<u64>()
            .map_err(|error| Error::Download(error.to_string()))
    }
}

pub async fn read_control_file(path: &Path) -> Result<Option<ControlFile>> {
    match fs::read_to_string(path).await {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|error| Error::Download(error.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Download(error.to_string())),
    }
}

pub async fn write_control_file_atomic(path: &Path, control: &ControlFile) -> Result<()> {
    let temp_path = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("raria")
    ));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|error| Error::Download(error.to_string()))?;
    }
    let bytes =
        serde_json::to_vec_pretty(control).map_err(|error| Error::Download(error.to_string()))?;
    let mut file = fs::File::create(&temp_path)
        .await
        .map_err(|error| Error::Download(error.to_string()))?;
    file.write_all(&bytes)
        .await
        .map_err(|error| Error::Download(error.to_string()))?;
    file.flush()
        .await
        .map_err(|error| Error::Download(error.to_string()))?;
    drop(file);
    fs::rename(&temp_path, path)
        .await
        .map_err(|error| Error::Download(error.to_string()))?;
    Ok(())
}
