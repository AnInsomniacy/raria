pub(crate) fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) fn parse_header_args(values: &[String]) -> anyhow::Result<Vec<(String, String)>> {
    values
        .iter()
        .map(|header| {
            let (name, value) = header
                .split_once(':')
                .ok_or_else(|| anyhow::anyhow!("invalid header '{header}': expected Name: Value"))?;
            let name = name.trim();
            let value = value.trim();
            anyhow::ensure!(!name.is_empty(), "invalid header '{header}': empty name");
            Ok((name.to_string(), value.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, parse_header_args};

    #[test]
    fn format_bytes_small() {
        assert_eq!(format_bytes(42), "42 B");
    }

    #[test]
    fn format_bytes_kib() {
        assert_eq!(format_bytes(2048), "2.00 KiB");
    }

    #[test]
    fn format_bytes_mib() {
        assert_eq!(format_bytes(1024 * 1024 * 5), "5.00 MiB");
    }

    #[test]
    fn format_bytes_gib() {
        assert_eq!(format_bytes(1024 * 1024 * 1024 * 2), "2.00 GiB");
    }

    #[test]
    fn parse_header_args_parses_pairs() {
        let headers = parse_header_args(&["X-Test: value".into()]).unwrap();
        assert_eq!(headers, vec![("X-Test".into(), "value".into())]);
    }

    #[test]
    fn parse_header_args_rejects_invalid_shape() {
        assert!(parse_header_args(&["broken".into()]).is_err());
    }
}
