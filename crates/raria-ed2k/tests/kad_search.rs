use raria_ed2k::kad::{
    KadContact, KadSearchEntry, KadTraversal, KadTraversalActionType, KadTraversalKind,
    build_kad_keyword_search_request, build_kad_publish_source_request, build_kad_search_result,
    build_kad_source_search_request, dedupe_kad_search_entries, extract_kad_source_entries,
    kad_keyword_target, parse_kad_publish_source_request, parse_kad_search_result,
    parse_kad_source_search_request,
};
use raria_ed2k::peer::PeerEndpoint;
use raria_ed2k::source::{SourceEndpoint, SourceLifecycle, SourceOrigin};
use raria_ed2k::tag::{Tag, TagName, TagValue};

fn id(value: u8) -> [u8; 16] {
    let mut id = [0; 16];
    id[0] = value;
    id
}

fn contact(value: u8) -> KadContact {
    KadContact {
        id: id(value),
        host: format!("203.0.113.{value}"),
        udp_port: 4672,
        tcp_port: 4662,
        version: 8,
        udp_key: Some(0x1122_3344),
        verified: true,
    }
}

#[test]
fn source_lookup_traversal_queries_nodes_then_searches_alive_contacts() {
    let target = id(0x44);
    let seed = contact(0x40);
    let closer = contact(0x41);
    let mut traversal = KadTraversal::new(KadTraversalKind::SourceLookup, target, 123_456, 1, 1);

    let actions = traversal.start(vec![seed.clone()]);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].action_type, KadTraversalActionType::FindNode);
    assert_eq!(actions[0].contact.id, seed.id);

    let actions = traversal.on_response(&seed, vec![closer.clone()]);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].action_type, KadTraversalActionType::Search);
    assert_eq!(actions[0].contact.id, seed.id);
    assert!(!traversal.is_done());

    let actions = traversal.on_response(&closer, Vec::new());
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].action_type, KadTraversalActionType::Search);
    assert_eq!(actions[0].contact.id, closer.id);

    let actions = traversal.on_response(&closer, Vec::new());
    assert!(actions.is_empty());
}

#[test]
fn source_search_payloads_merge_direct_kad_sources_through_source_policy() {
    let target = id(0x55);
    let request = build_kad_source_search_request(target, 7, 9_728_001);
    let parsed_request = parse_kad_source_search_request(&request).expect("source request");
    assert_eq!(parsed_request.target_id, target);
    assert_eq!(parsed_request.start_position, 7);
    assert_eq!(parsed_request.file_size, 9_728_001);

    let source = KadSearchEntry {
        id: id(0xaa),
        tags: vec![
            Tag::new(TagName::Id(0xff), TagValue::UInt32(1)),
            Tag::new(TagName::Id(0xfe), TagValue::UInt32(0x0403_0201)),
            Tag::new(TagName::Id(0xfd), TagValue::UInt32(4662)),
            Tag::new(TagName::Id(0xfc), TagValue::UInt32(4672)),
            Tag::new(TagName::Id(0xf3), TagValue::UInt32(1)),
        ],
    };
    let payload = build_kad_search_result(id(0x99), target, &[source]).expect("result");
    let result = parse_kad_search_result(&payload).expect("parsed result");
    let entries = extract_kad_source_entries(&result);

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].endpoint.ip, 0x0102_0304);
    assert_eq!(entries[0].endpoint.port, 4662);
    assert_eq!(entries[0].user_hash, Some(id(0xaa)));

    let mut lifecycle = SourceLifecycle::new(SourceEndpoint::new(0x0a0b_0c0d, 4662), 10);
    assert!(lifecycle.merge(entries[0].clone(), SourceOrigin::Kad, 100));
    assert_eq!(lifecycle.sources()[0].endpoint.ip, 0x0102_0304);
}

#[test]
fn source_publish_respects_sharing_policy_and_large_file_source_type() {
    let file_id = id(0x66);
    let source_id = id(0xbb);
    let source = PeerEndpoint {
        ip: 0x0102_0304,
        port: 4662,
    };

    assert!(
        build_kad_publish_source_request(file_id, source, source_id, 10, false)
            .unwrap()
            .is_none()
    );

    let payload =
        build_kad_publish_source_request(file_id, source, source_id, u64::from(u32::MAX) + 1, true)
            .unwrap()
            .expect("publish payload");
    let parsed = parse_kad_publish_source_request(&payload).expect("publish request");
    assert_eq!(parsed.file_id, file_id);
    assert_eq!(parsed.source.id, source_id);

    let result = build_kad_search_result(id(0x99), file_id, &[parsed.source]).expect("result");
    let entries = extract_kad_source_entries(&parse_kad_search_result(&result).unwrap());
    assert_eq!(entries[0].endpoint, source);
}

#[test]
fn keyword_target_and_request_are_stable_and_result_tags_are_deduplicable() {
    let first = kad_keyword_target("small Linux ISO").expect("target");
    let second = kad_keyword_target("SMALL-linux.iso").expect("target");
    assert_eq!(first, second);
    assert!(kad_keyword_target("a b c").is_err());

    let request = build_kad_keyword_search_request(first, 3);
    assert_eq!(&request[..16], &first);
    assert_eq!(u16::from_le_bytes([request[16], request[17]]), 3);

    let duplicate_a = KadSearchEntry {
        id: id(0xcc),
        tags: vec![
            Tag::new(TagName::Id(0x01), TagValue::String("file.iso".to_string())),
            Tag::new(TagName::Id(0x02), TagValue::UInt32(1024)),
            Tag::new(TagName::Id(0x15), TagValue::UInt32(3)),
        ],
    };
    let duplicate_b = KadSearchEntry {
        id: id(0xcc),
        tags: vec![
            Tag::new(TagName::Id(0x01), TagValue::String("file.iso".to_string())),
            Tag::new(TagName::Id(0x02), TagValue::UInt32(1024)),
            Tag::new(TagName::Id(0x15), TagValue::UInt32(5)),
        ],
    };
    let result = parse_kad_search_result(
        &build_kad_search_result(id(0x99), first, &[duplicate_a, duplicate_b]).unwrap(),
    )
    .unwrap();

    let deduped = dedupe_kad_search_entries(result.entries);
    assert_eq!(deduped.len(), 1);
    assert_eq!(deduped[0].id, id(0xcc));
}
