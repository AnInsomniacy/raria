use raria_ed2k::search::{
    Ed2kServerSearchQuery, Ed2kServerSearchResult, SearchResultSource, build_server_search_request,
    build_server_search_result_payload, parse_server_search_results,
};

fn hash(value: u8) -> [u8; 16] {
    [value; 16]
}

#[test]
fn server_search_request_encodes_retained_filters() {
    let payload = build_server_search_request(&Ed2kServerSearchQuery {
        keyword: "test linux".to_string(),
        file_type: Some("Pro".to_string()),
        extension: Some("iso".to_string()),
        min_size: Some(1024),
        max_size: Some(u64::from(u32::MAX) + 9),
        min_source_count: Some(3),
        min_complete_source_count: Some(2),
    })
    .expect("search request");

    assert!(payload.windows(10).any(|window| window == b"test linux"));
    assert!(payload.contains(&0x03));
    assert!(payload.contains(&0x04));
    assert!(payload.contains(&0x15));
    assert!(payload.contains(&0x30));
    assert!(payload.contains(&0x08));
}

#[test]
fn server_search_results_parse_metadata_sources_and_links() {
    let entry = Ed2kServerSearchResult {
        hash: hash(0x11),
        name: "sample.iso".to_string(),
        size: u64::from(u32::MAX) + 17,
        source_count: 9,
        complete_source_count: 4,
        file_type: Some("Pro".to_string()),
        extension: Some("iso".to_string()),
        source_network: "server".to_string(),
        sources: vec![SearchResultSource {
            host: "1.2.3.4".to_string(),
            port: 4662,
        }],
        ed2k_uri: String::new(),
    };

    let payload = build_server_search_result_payload(&[entry]).expect("payload");
    let parsed = parse_server_search_results(&payload, "server").expect("results");

    assert!(!parsed.more_results);
    assert_eq!(parsed.entries.len(), 1);
    let result = &parsed.entries[0];
    assert_eq!(result.hash, hash(0x11));
    assert_eq!(result.name, "sample.iso");
    assert_eq!(result.size, u64::from(u32::MAX) + 17);
    assert_eq!(result.source_count, 9);
    assert_eq!(result.complete_source_count, 4);
    assert_eq!(result.file_type.as_deref(), Some("Pro"));
    assert_eq!(result.extension.as_deref(), Some("iso"));
    assert_eq!(result.source_network, "server");
    assert_eq!(
        result.sources,
        vec![SearchResultSource {
            host: "1.2.3.4".to_string(),
            port: 4662
        }]
    );
    assert_eq!(
        result.ed2k_uri,
        "ed2k://|file|sample.iso|4294967312|11111111111111111111111111111111|sources,1.2.3.4:4662|/"
    );
}

#[test]
fn server_search_result_links_escape_unsafe_names() {
    let entry = Ed2kServerSearchResult {
        hash: hash(0x22),
        name: "unsafe/name with space.bin".to_string(),
        size: 12,
        source_count: 1,
        complete_source_count: 1,
        file_type: None,
        extension: None,
        source_network: "server".to_string(),
        sources: Vec::new(),
        ed2k_uri: String::new(),
    };

    let payload = build_server_search_result_payload(&[entry]).expect("payload");
    let parsed = parse_server_search_results(&payload, "server").expect("results");

    assert_eq!(
        parsed.entries[0].ed2k_uri,
        "ed2k://|file|unsafe%2Fname%20with%20space.bin|12|22222222222222222222222222222222|/"
    );
}

#[test]
fn malformed_server_search_data_fails_without_partial_results() {
    assert!(build_server_search_request(&Ed2kServerSearchQuery::default()).is_err());
    assert!(parse_server_search_results(&[], "server").is_err());

    let mut malformed = Vec::new();
    malformed.extend_from_slice(&1_u32.to_le_bytes());
    malformed.extend_from_slice(&hash(0x33));
    assert!(parse_server_search_results(&malformed, "server").is_err());
}
