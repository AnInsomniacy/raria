use raria_ed2k::kad::{
    KadContact, KadSearchEntry, build_kad_search_result, parse_kad_publish_source_request,
    parse_kad_source_search_request,
};
use raria_ed2k::opcode::KadOpcode;
use raria_ed2k::packet::{PacketFrame, Protocol, decode_udp_datagram, encode_udp_datagram};
use raria_ed2k::peer::PeerEndpoint;
use raria_ed2k::runtime::{
    Ed2kKadRuntime, Ed2kKadRuntimeConfig, Ed2kKadSourceLookup, Ed2kKadSourcePublish,
    Ed2kKeywordLookup,
};
use raria_ed2k::tag::{Tag, TagName, TagValue};
use tokio::net::UdpSocket;

const MAX_PACKET: usize = 16 * 1024;

fn id(value: u8) -> [u8; 16] {
    let mut id = [0; 16];
    id[0] = value;
    id
}

fn contact(value: u8, udp_port: u16) -> KadContact {
    KadContact {
        id: id(value),
        host: "127.0.0.1".to_string(),
        udp_port,
        tcp_port: 4662,
        version: 8,
        udp_key: Some(0x1122_3344),
        verified: true,
    }
}

#[tokio::test]
async fn kad_runtime_bootstraps_contact_and_collects_sources() {
    let server = UdpSocket::bind("127.0.0.1:0").await.expect("server");
    let server_addr = server.local_addr().expect("server addr");
    let target = id(0x55);
    let remote = contact(0x44, server_addr.port());
    let remote_for_task = remote.clone();
    let server_task = tokio::spawn(async move {
        let (hello, peer) = recv_frame(&server).await;
        assert_eq!(hello.opcode, u8::from(KadOpcode::HelloRequestV2));
        send_frame(
            &server,
            peer,
            KadOpcode::HelloResponseV2,
            remote_for_task.id.to_vec(),
        )
        .await;

        let (search, peer) = recv_frame(&server).await;
        assert_eq!(search.opcode, u8::from(KadOpcode::SearchSourceRequestV2));
        let parsed = parse_kad_source_search_request(&search.payload).expect("source request");
        assert_eq!(parsed.target_id, target);
        let entry = KadSearchEntry {
            id: id(0xaa),
            tags: vec![
                Tag::new(TagName::Id(0xff), TagValue::UInt32(1)),
                Tag::new(TagName::Id(0xfe), TagValue::UInt32(0x0403_0201)),
                Tag::new(TagName::Id(0xfd), TagValue::UInt32(4662)),
            ],
        };
        send_frame(
            &server,
            peer,
            KadOpcode::SearchResponseV2,
            build_kad_search_result(remote_for_task.id, target, &[entry]).expect("result"),
        )
        .await;
    });

    let mut runtime = Ed2kKadRuntime::new(Ed2kKadRuntimeConfig {
        self_id: id(0x11),
        udp_bind_addr: "127.0.0.1:0".to_string(),
        ..Default::default()
    });
    let report = runtime
        .lookup_sources(
            Ed2kKadSourceLookup {
                target_id: target,
                file_size: 700,
                seeds: vec![remote.clone()],
            },
            100,
        )
        .await
        .expect("lookup");

    server_task.await.expect("server task");
    assert_eq!(report.confirmed_contacts, 1);
    assert_eq!(report.sources.len(), 1);
    assert_eq!(
        report.sources[0].endpoint,
        PeerEndpoint {
            ip: 0x0102_0304,
            port: 4662
        }
    );
    assert_eq!(
        runtime.routing().find_closest(&remote.id, 1, false)[0].id,
        remote.id
    );
}

#[tokio::test]
async fn kad_runtime_runs_keyword_search_and_dedupes_results() {
    let server = UdpSocket::bind("127.0.0.1:0").await.expect("server");
    let server_addr = server.local_addr().expect("server addr");
    let target = id(0x77);
    let remote = contact(0x45, server_addr.port());
    let remote_for_task = remote.clone();
    let server_task = tokio::spawn(async move {
        let (search, peer) = recv_frame(&server).await;
        assert_eq!(search.opcode, u8::from(KadOpcode::SearchKeyRequestV2));
        assert_eq!(&search.payload[..16], &target);
        let duplicate_a = KadSearchEntry {
            id: id(0xbb),
            tags: vec![Tag::new(
                TagName::Id(0x01),
                TagValue::String("file.iso".to_string()),
            )],
        };
        let duplicate_b = KadSearchEntry {
            id: id(0xbb),
            tags: vec![Tag::new(
                TagName::Id(0x01),
                TagValue::String("file.iso".to_string()),
            )],
        };
        send_frame(
            &server,
            peer,
            KadOpcode::SearchResponseV2,
            build_kad_search_result(remote_for_task.id, target, &[duplicate_a, duplicate_b])
                .expect("result"),
        )
        .await;
    });

    let runtime = Ed2kKadRuntime::new(Ed2kKadRuntimeConfig {
        self_id: id(0x12),
        udp_bind_addr: "127.0.0.1:0".to_string(),
        ..Default::default()
    });
    let report = runtime
        .lookup_keyword(
            Ed2kKeywordLookup {
                target_id: target,
                contacts: vec![remote],
            },
            101,
        )
        .await
        .expect("keyword");

    server_task.await.expect("server task");
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].id, id(0xbb));
}

#[tokio::test]
async fn kad_runtime_skips_unresponsive_contacts() {
    let silent = UdpSocket::bind("127.0.0.1:0").await.expect("silent");
    let silent_addr = silent.local_addr().expect("silent addr");
    let target = id(0x88);
    let remote = contact(0x47, silent_addr.port());
    let mut runtime = Ed2kKadRuntime::new(Ed2kKadRuntimeConfig {
        self_id: id(0x14),
        io_timeout: std::time::Duration::from_millis(10),
        udp_bind_addr: "127.0.0.1:0".to_string(),
        ..Default::default()
    });

    let report = runtime
        .lookup_sources(
            Ed2kKadSourceLookup {
                target_id: target,
                file_size: 700,
                seeds: vec![remote.clone()],
            },
            103,
        )
        .await
        .expect("lookup should skip silent contacts");

    assert_eq!(report.confirmed_contacts, 0);
    assert!(report.sources.is_empty());
    assert!(
        runtime
            .routing()
            .find_closest(&remote.id, 1, true)
            .is_empty()
    );
}

#[tokio::test]
async fn kad_runtime_publishes_source_and_records_firewall_state() {
    let server = UdpSocket::bind("127.0.0.1:0").await.expect("server");
    let server_addr = server.local_addr().expect("server addr");
    let file_id = id(0x99);
    let remote = contact(0x46, server_addr.port());
    let server_task = tokio::spawn(async move {
        let (publish, peer) = recv_frame(&server).await;
        assert_eq!(publish.opcode, u8::from(KadOpcode::PublishSourceRequestV2));
        let parsed = parse_kad_publish_source_request(&publish.payload).expect("publish");
        assert_eq!(parsed.file_id, file_id);
        send_frame(&server, peer, KadOpcode::PublishResponseV2, Vec::new()).await;

        let (firewall, peer) = recv_frame(&server).await;
        assert_eq!(firewall.opcode, u8::from(KadOpcode::FirewalledRequestV2));
        send_frame(&server, peer, KadOpcode::FirewalledResponse, Vec::new()).await;
    });

    let mut runtime = Ed2kKadRuntime::new(Ed2kKadRuntimeConfig {
        self_id: id(0x13),
        udp_bind_addr: "127.0.0.1:0".to_string(),
        ..Default::default()
    });
    let report = runtime
        .publish_source(
            Ed2kKadSourcePublish {
                file_id,
                file_size: 700,
                source: PeerEndpoint {
                    ip: 0x0102_0304,
                    port: 4662,
                },
                source_id: id(0xcc),
                contact: remote,
                sharing_enabled: true,
            },
            102,
        )
        .await
        .expect("publish");

    server_task.await.expect("server task");
    assert!(report.published);
    assert!(report.udp_reachable);
    assert!(runtime.firewall().udp_reachable());
}

async fn recv_frame(socket: &UdpSocket) -> (PacketFrame, std::net::SocketAddr) {
    let mut buf = [0_u8; 2048];
    let (len, peer) = socket.recv_from(&mut buf).await.expect("recv");
    (
        decode_udp_datagram(&buf[..len], MAX_PACKET).expect("frame"),
        peer,
    )
}

async fn send_frame(
    socket: &UdpSocket,
    peer: std::net::SocketAddr,
    opcode: KadOpcode,
    payload: Vec<u8>,
) {
    let bytes = encode_udp_datagram(
        &PacketFrame {
            protocol: Protocol::Kad,
            opcode: opcode.into(),
            payload,
        },
        MAX_PACKET,
    )
    .expect("encode");
    socket.send_to(&bytes, peer).await.expect("send");
}
