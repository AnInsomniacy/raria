use raria_ed2k::opcode::{KadOpcode, PeerOpcode, ServerOpcode};
use raria_ed2k::packet::{
    PacketError, PacketFrame, Protocol, decode_tcp_frame, decode_udp_datagram, encode_tcp_frame,
    encode_udp_datagram, pack_payload, unpack_payload,
};
use raria_ed2k::tag::{Tag, TagName, TagValue, decode_tag, encode_tag};

#[test]
fn tcp_frame_roundtrips_with_little_endian_header() {
    let frame = PacketFrame {
        protocol: Protocol::Edonkey,
        opcode: ServerOpcode::LoginRequest.into(),
        payload: vec![0xaa, 0xbb, 0xcc],
    };

    let encoded = encode_tcp_frame(&frame, 1024).expect("encoded frame");

    assert_eq!(encoded, vec![0xe3, 4, 0, 0, 0, 0x01, 0xaa, 0xbb, 0xcc]);
    assert_eq!(
        decode_tcp_frame(&encoded, 1024).expect("decoded frame"),
        frame
    );
}

#[test]
fn tcp_frame_rejects_truncated_and_oversized_payloads() {
    assert_eq!(
        decode_tcp_frame(&[0xe3, 1, 0, 0, 0], 1024),
        Err(PacketError::Truncated)
    );
    assert_eq!(
        decode_tcp_frame(&[0xe3, 0, 0, 0, 0, 0x01], 1024),
        Err(PacketError::InvalidLength)
    );
    assert_eq!(
        decode_tcp_frame(&[0xe3, 5, 0, 0, 0, 0x01, 1, 2, 3, 4], 3),
        Err(PacketError::PayloadTooLarge { size: 4, max: 3 })
    );
}

#[test]
fn udp_datagram_roundtrips_kad_packet() {
    let frame = PacketFrame {
        protocol: Protocol::Kad,
        opcode: KadOpcode::HelloRequestV2.into(),
        payload: vec![0x10, 0x20],
    };

    let encoded = encode_udp_datagram(&frame, 1024).expect("encoded datagram");

    assert_eq!(encoded, vec![0xe4, 0x11, 0x10, 0x20]);
    assert_eq!(
        decode_udp_datagram(&encoded, 1024).expect("decoded datagram"),
        frame
    );
}

#[test]
fn packed_payload_roundtrips_and_respects_output_limit() {
    let payload = b"aaaaaaaaaabbbbbbbbbbccccccccccdddddddddd";
    let packed = pack_payload(payload).expect("packed payload");

    assert_eq!(
        unpack_payload(&packed, payload.len()).expect("unpacked payload"),
        payload
    );
    assert!(matches!(
        unpack_payload(&packed, payload.len() - 1),
        Err(PacketError::PayloadTooLarge { .. })
    ));
}

#[test]
fn typed_tags_roundtrip_with_stable_widths() {
    let tags = [
        Tag::new(TagName::Id(0x01), TagValue::String("file.iso".into())),
        Tag::new(TagName::Id(0x02), TagValue::UInt8(7)),
        Tag::new(TagName::Text("size".into()), TagValue::UInt16(700)),
        Tag::new(TagName::Id(0x03), TagValue::UInt32(70_000)),
        Tag::new(TagName::Id(0x04), TagValue::UInt64(5_000_000_000)),
        Tag::new(TagName::Id(0x05), TagValue::Bool(true)),
        Tag::new(TagName::Id(0x06), TagValue::Hash([0x11; 16])),
        Tag::new(
            TagName::Text("blob".into()),
            TagValue::Binary(vec![1, 2, 3]),
        ),
    ];

    for tag in tags {
        let encoded = encode_tag(&tag).expect("encoded tag");
        assert_eq!(decode_tag(&encoded).expect("decoded tag"), tag);
    }
}

#[test]
fn retained_opcodes_are_named_and_legacy_chat_is_not() {
    assert_eq!(
        ServerOpcode::from_byte(0x19),
        Some(ServerOpcode::GetSources)
    );
    assert_eq!(PeerOpcode::from_byte(0x47), Some(PeerOpcode::RequestParts));
    assert_eq!(
        KadOpcode::from_byte(0x34),
        Some(KadOpcode::SearchSourceRequestV2)
    );
    assert_eq!(ServerOpcode::from_byte(0x1e), None);
}
