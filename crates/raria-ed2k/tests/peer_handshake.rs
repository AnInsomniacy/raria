use raria_ed2k::opcode::{EmuleOpcode, PeerOpcode};
use raria_ed2k::packet::Protocol;
use raria_ed2k::peer::{
    PeerEndpoint, PeerHandshakeError, PeerIdentity, build_emule_info, build_peer_hello,
    build_peer_hello_answer, parse_emule_info, parse_peer_hello,
};

#[test]
fn peer_hello_and_answer_carry_native_identity_and_truthful_capabilities() {
    let identity = PeerIdentity {
        user_hash: [0x11; 16],
        client_id: 0x0102_0304,
        tcp_port: 4662,
        udp_port: 4672,
        kad_udp_port: 0,
        server: Some(PeerEndpoint {
            ip: 0x0506_0708,
            port: 4661,
        }),
        name: "raria".into(),
    };

    let hello = build_peer_hello(&identity).expect("hello");
    assert_eq!(hello.protocol, Protocol::Edonkey);
    assert_eq!(hello.opcode, u8::from(PeerOpcode::Hello));

    let parsed = parse_peer_hello(&hello).expect("parsed hello");
    assert_eq!(parsed.identity.user_hash, [0x11; 16]);
    assert_eq!(parsed.identity.client_id, 0x0102_0304);
    assert_eq!(parsed.identity.tcp_port, 4662);
    assert_eq!(parsed.identity.udp_port, 4672);
    assert_eq!(parsed.identity.server, identity.server);
    assert_eq!(parsed.identity.name, "raria");
    assert_eq!(parsed.capabilities.aich_version, 1);
    assert!(parsed.capabilities.unicode);
    assert_eq!(parsed.capabilities.data_compression_version, 1);
    assert_eq!(parsed.capabilities.source_exchange1_version, 3);
    assert_eq!(parsed.capabilities.extended_requests_version, 2);
    assert!(parsed.capabilities.supports_large_files);
    assert!(parsed.capabilities.supports_source_exchange2);
    assert!(!parsed.capabilities.supports_crypt_layer);
    assert!(!parsed.capabilities.requests_crypt_layer);
    assert!(!parsed.capabilities.requires_crypt_layer);
    assert_eq!(parsed.capabilities.secure_ident_version, 0);
    assert_eq!(parsed.capabilities.kad_version, 0);
    assert!(!parsed.capabilities.supports_multipacket);
    assert!(!parsed.capabilities.supports_extended_multipacket);
    assert!(!parsed.capabilities.supports_direct_udp_callback);
    assert!(!parsed.capabilities.supports_captcha);
    assert!(!parsed.capabilities.accepts_comments);
    assert!(!parsed.capabilities.supports_preview);

    let answer = build_peer_hello_answer(&identity).expect("hello answer");
    assert_eq!(answer.protocol, Protocol::Edonkey);
    assert_eq!(answer.opcode, u8::from(PeerOpcode::HelloAnswer));
    assert_eq!(
        parse_peer_hello(&answer).expect("parsed answer").identity,
        identity
    );
}

#[test]
fn emule_info_advertises_only_owned_legacy_info_capabilities() {
    let frame = build_emule_info(4672, false).expect("info");
    assert_eq!(frame.protocol, Protocol::Emule);
    assert_eq!(frame.opcode, u8::from(EmuleOpcode::Info));

    let parsed = parse_emule_info(&frame).expect("parsed info");
    assert_eq!(parsed.udp_port, 4672);
    assert_eq!(parsed.capabilities.data_compression_version, 1);
    assert_eq!(parsed.capabilities.source_exchange1_version, 3);
    assert_eq!(parsed.capabilities.extended_requests_version, 2);
    assert_eq!(parsed.capabilities.secure_ident_version, 0);
    assert!(!parsed.capabilities.accepts_comments);
    assert!(!parsed.capabilities.supports_preview);

    let answer = build_emule_info(4672, true).expect("info answer");
    assert_eq!(answer.protocol, Protocol::Emule);
    assert_eq!(answer.opcode, u8::from(EmuleOpcode::InfoAnswer));
    assert_eq!(
        parse_emule_info(&answer).expect("parsed answer").udp_port,
        4672
    );
}

#[test]
fn malformed_peer_handshake_fails_without_partial_state() {
    let mut identity = PeerIdentity::default_for_name("raria");
    identity.user_hash = [0x22; 16];
    let mut frame = build_peer_hello(&identity).expect("hello");

    frame.payload[0] = 15;
    assert_eq!(
        parse_peer_hello(&frame),
        Err(PeerHandshakeError::InvalidHashSize(15))
    );

    frame.payload[0] = 16;
    frame.payload.truncate(12);
    assert_eq!(parse_peer_hello(&frame), Err(PeerHandshakeError::Truncated));

    frame.opcode = u8::from(PeerOpcode::SendingPart);
    assert_eq!(
        parse_peer_hello(&frame),
        Err(PeerHandshakeError::UnexpectedOpcode(frame.opcode))
    );
}
