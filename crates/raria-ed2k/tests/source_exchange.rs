use raria_ed2k::opcode::PeerOpcode;
use raria_ed2k::packet::Protocol;
use raria_ed2k::peer::{PeerCapabilities, PeerEndpoint, PeerRequestPhase};
use raria_ed2k::source::{
    SourceEndpoint, SourceExchangeEntry, SourceExchangeError, SourceLifecycle, SourceOrigin,
    SourceQuality, build_source_exchange_answer, build_source_exchange_request,
    parse_source_exchange_answer, source_exchange_request_version,
};

#[test]
fn source_exchange_requests_and_answers_roundtrip_sx1_and_sx2_metadata() {
    let hash = [0x11; 16];
    let mut caps = PeerCapabilities {
        source_exchange1_version: 3,
        ..Default::default()
    };
    let sx1 = build_source_exchange_request(hash, caps).expect("sx1 request");
    assert_eq!(sx1.protocol, Protocol::Emule);
    assert_eq!(sx1.opcode, u8::from(PeerOpcode::RequestSources));
    assert_eq!(sx1.payload, hash);
    assert_eq!(source_exchange_request_version(&sx1, hash).unwrap(), 1);

    caps.supports_source_exchange2 = true;
    let sx2 = build_source_exchange_request(hash, caps).expect("sx2 request");
    assert_eq!(sx2.opcode, u8::from(PeerOpcode::RequestSources2));
    assert_eq!(sx2.payload[0], 4);
    assert_eq!(&sx2.payload[3..], &hash);
    assert_eq!(source_exchange_request_version(&sx2, hash).unwrap(), 4);

    let entries = vec![
        SourceExchangeEntry {
            endpoint: PeerEndpoint {
                ip: 0x0102_0304,
                port: 4662,
            },
            server: Some(PeerEndpoint {
                ip: 0x0506_0708,
                port: 4661,
            }),
            user_hash: Some([0xaa; 16]),
            crypt_options: Some(0x01),
        },
        SourceExchangeEntry {
            endpoint: PeerEndpoint {
                ip: 0x1112_1314,
                port: 4663,
            },
            server: None,
            user_hash: Some([0xbb; 16]),
            crypt_options: Some(0x04),
        },
    ];
    let answer = build_source_exchange_answer(hash, 4, true, &entries).expect("answer");
    assert_eq!(answer.opcode, u8::from(PeerOpcode::AnswerSources2));

    let parsed = parse_source_exchange_answer(&answer, hash, None).expect("parsed answer");
    assert_eq!(parsed.version, 4);
    assert_eq!(parsed.entries, entries);
    assert!(parsed.entries[0].is_schedulable_without_crypt());
    assert!(!parsed.entries[1].is_schedulable_without_crypt());

    let sx1_answer = build_source_exchange_answer(hash, 1, false, &entries).expect("sx1 answer");
    assert_eq!(sx1_answer.opcode, u8::from(PeerOpcode::AnswerSources));
    let parsed_sx1 = parse_source_exchange_answer(&sx1_answer, hash, Some(1)).unwrap();
    assert_eq!(parsed_sx1.entries[0].user_hash, None);
    assert_eq!(parsed_sx1.entries[0].crypt_options, None);
}

#[test]
fn source_lifecycle_merges_sources_and_preserves_backoff_state() {
    let mut lifecycle = SourceLifecycle::new(SourceEndpoint::new(0x0a0b_0c0d, 4662), 2);

    let inserted = lifecycle.merge(
        SourceExchangeEntry {
            endpoint: PeerEndpoint {
                ip: 0x0102_0304,
                port: 4662,
            },
            server: None,
            user_hash: Some([0xaa; 16]),
            crypt_options: Some(0x01),
        },
        SourceOrigin::SourceExchange,
        100,
    );
    assert!(inserted);
    assert_eq!(lifecycle.len(), 1);
    assert_eq!(
        lifecycle.next_schedulable(101).unwrap().quality,
        SourceQuality::Fresh
    );

    let duplicate = lifecycle.merge(
        SourceExchangeEntry {
            endpoint: PeerEndpoint {
                ip: 0x0102_0304,
                port: 4662,
            },
            server: Some(PeerEndpoint {
                ip: 0x0506_0708,
                port: 4661,
            }),
            user_hash: Some([0xaa; 16]),
            crypt_options: Some(0x04),
        },
        SourceOrigin::Server,
        110,
    );
    assert!(!duplicate);
    assert_eq!(lifecycle.len(), 1);
    assert_eq!(lifecycle.sources()[0].origin, SourceOrigin::Server);
    assert_eq!(lifecycle.sources()[0].server.unwrap().ip, 0x0506_0708);

    lifecycle.mark_queued(SourceEndpoint::new(0x0102_0304, 4662), 50, 120);
    assert!(lifecycle.next_schedulable(121).is_none());
    assert_eq!(
        lifecycle.next_schedulable(171).unwrap().quality,
        SourceQuality::Queued
    );

    lifecycle.mark_dead(SourceEndpoint::new(0x0102_0304, 4662), 200, 30);
    assert!(lifecycle.next_schedulable(220).is_none());
    assert_eq!(
        lifecycle.next_schedulable(231).unwrap().quality,
        SourceQuality::Recovered
    );
}

#[test]
fn source_policy_rejects_self_loopback_required_crypt_and_active_cap_overflow() {
    let mut lifecycle = SourceLifecycle::new(SourceEndpoint::new(0x0102_0304, 4662), 1);

    for endpoint in [
        PeerEndpoint { ip: 0, port: 4662 },
        PeerEndpoint {
            ip: 0x0102_0304,
            port: 4662,
        },
        PeerEndpoint {
            ip: 0x7f00_0001,
            port: 4662,
        },
    ] {
        assert!(!lifecycle.merge(
            SourceExchangeEntry {
                endpoint,
                server: None,
                user_hash: None,
                crypt_options: None,
            },
            SourceOrigin::SourceExchange,
            10,
        ));
    }

    assert!(lifecycle.merge(
        SourceExchangeEntry {
            endpoint: PeerEndpoint {
                ip: 0x1112_1314,
                port: 4662,
            },
            server: None,
            user_hash: Some([0xaa; 16]),
            crypt_options: Some(0x01),
        },
        SourceOrigin::Inline,
        20,
    ));
    assert!(!lifecycle.merge(
        SourceExchangeEntry {
            endpoint: PeerEndpoint {
                ip: 0x2122_2324,
                port: 4662,
            },
            server: None,
            user_hash: Some([0xbb; 16]),
            crypt_options: Some(0x04),
        },
        SourceOrigin::SourceExchange,
        20,
    ));

    let selected = lifecycle.next_schedulable(21).expect("selected source");
    assert_eq!(selected.origin, SourceOrigin::Inline);
    let selected_endpoint = selected.endpoint;
    lifecycle.mark_active(selected_endpoint);
    assert!(lifecycle.next_schedulable(22).is_none());

    lifecycle.update_phase(selected_endpoint, PeerRequestPhase::NoNeededParts, 25);
    assert_eq!(
        lifecycle.next_schedulable(26).unwrap().quality,
        SourceQuality::NoNeededParts
    );
}

#[test]
fn malformed_source_exchange_payloads_return_typed_errors() {
    let hash = [0x55; 16];
    let wrong_hash = [0x66; 16];
    let answer = build_source_exchange_answer(hash, 4, true, &[]).expect("answer");

    assert_eq!(
        parse_source_exchange_answer(&answer, wrong_hash, None),
        Err(SourceExchangeError::HashMismatch)
    );

    let mut truncated = answer.clone();
    truncated.payload.pop();
    assert_eq!(
        parse_source_exchange_answer(&truncated, hash, None),
        Err(SourceExchangeError::Truncated)
    );

    let caps = PeerCapabilities {
        source_exchange1_version: 1,
        ..Default::default()
    };
    assert_eq!(build_source_exchange_request(hash, caps), None);
}
