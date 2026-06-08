use quick_xml::{
    Reader,
    events::{BytesStart, Event},
};

use crate::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetalinkDocument {
    pub files: Vec<MetalinkFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetalinkFile {
    pub name: String,
    pub size: Option<u64>,
    pub checksum: Option<String>,
    pub resources: Vec<String>,
}

pub fn parse_metalink_bytes(bytes: &[u8]) -> Result<MetalinkDocument> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);

    let mut files = Vec::new();
    let mut current_file: Option<MetalinkFile> = None;
    let mut current_text: Option<TextTarget> = None;

    loop {
        match reader
            .read_event()
            .map_err(|error| Error::Download(format!("invalid Metalink XML: {error}")))?
        {
            Event::Start(element) => match local_name(element.name().as_ref()) {
                b"file" => {
                    current_file = Some(MetalinkFile {
                        name: attribute(&element, b"name")?.ok_or_else(|| {
                            Error::Download("Metalink file is missing name".into())
                        })?,
                        size: None,
                        checksum: None,
                        resources: Vec::new(),
                    });
                }
                b"size" if current_file.is_some() => {
                    current_text = Some(TextTarget::Size);
                }
                b"hash" if current_file.is_some() => {
                    let kind = attribute(&element, b"type")?
                        .unwrap_or_else(|| "sha-256".to_string())
                        .to_ascii_lowercase();
                    current_text = Some(TextTarget::Hash(kind));
                }
                b"url" if current_file.is_some() => {
                    current_text = Some(TextTarget::Url);
                }
                _ => {}
            },
            Event::Text(text) => {
                let Some(target) = current_text.take() else {
                    continue;
                };
                let value = text
                    .unescape()
                    .map_err(|error| Error::Download(error.to_string()))?
                    .trim()
                    .to_string();
                if value.is_empty() {
                    continue;
                }
                let Some(file) = current_file.as_mut() else {
                    continue;
                };
                match target {
                    TextTarget::Size => {
                        file.size = Some(
                            value
                                .parse::<u64>()
                                .map_err(|error| Error::Download(error.to_string()))?,
                        );
                    }
                    TextTarget::Hash(kind) => {
                        if kind == "sha-256" || kind == "sha256" {
                            file.checksum = Some(format!("sha-256={value}"));
                        }
                    }
                    TextTarget::Url => {
                        file.resources.push(value);
                    }
                }
            }
            Event::End(element) => match local_name(element.name().as_ref()) {
                b"file" => {
                    if let Some(file) = current_file.take() {
                        files.push(file);
                    }
                    current_text = None;
                }
                b"size" | b"hash" | b"url" => {
                    current_text = None;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(MetalinkDocument { files })
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TextTarget {
    Size,
    Hash(String),
    Url,
}

fn attribute(element: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| Error::Download(error.to_string()))?;
        if local_name(attribute.key.as_ref()) == name {
            return Ok(Some(
                attribute
                    .unescape_value()
                    .map_err(|error| Error::Download(error.to_string()))?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}
