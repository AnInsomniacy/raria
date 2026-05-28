use std::net::SocketAddr;
use std::time::Duration;

use raria_ed2k::opcode::PeerOpcode;
use raria_ed2k::packet::{PacketFrame, Protocol, decode_tcp_frame, encode_tcp_frame};
use raria_ed2k::peer::{
    PeerEndpoint, PeerIdentity, PeerRequestPhase, build_emule_info, build_file_status_answer,
    build_hashset_answer, build_peer_hello_answer, parse_peer_hello,
};
use raria_ed2k::runtime::{Ed2kPeerDownloadRequest, Ed2kPeerRuntime, Ed2kPeerRuntimeConfig};
use raria_ed2k::source::{SourceExchangeEntry, build_source_exchange_answer};
use raria_ed2k::transfer::{PartRange, TransferStatus, parse_part_request};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const MAX_PACKET: usize = 16 * 1024;

fn id(value: u8) -> [u8; 16] {
    let mut id = [0; 16];
    id[0] = value;
    id
}

#[tokio::test]
async fn peer_runtime_handshakes_downloads_part_and_merges_sources() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("listener addr");
    let file_hash = id(0x55);
    let server_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let hello = read_frame(&mut stream).await;
        assert_eq!(hello.opcode, u8::from(PeerOpcode::Hello));
        let parsed_hello = parse_peer_hello(&hello).expect("hello");
        assert_eq!(parsed_hello.identity.name, "raria-test");

        write_frame(
            &mut stream,
            &build_peer_hello_answer(&remote_identity()).expect("hello answer"),
        )
        .await;

        let info = read_frame(&mut stream).await;
        assert_eq!(info.protocol, Protocol::Emule);
        write_frame(
            &mut stream,
            &build_emule_info(remote_identity().udp_port, true).expect("info answer"),
        )
        .await;

        let source_request = read_frame(&mut stream).await;
        assert_eq!(source_request.opcode, u8::from(PeerOpcode::RequestSources));
        let source = SourceExchangeEntry {
            endpoint: PeerEndpoint {
                ip: 0x0403_0201,
                port: 4662,
            },
            server: None,
            user_hash: Some(id(0x66)),
            crypt_options: None,
        };
        write_frame(
            &mut stream,
            &build_source_exchange_answer(file_hash, 3, false, &[source.clone(), source])
                .expect("source answer"),
        )
        .await;

        let file_status = read_frame(&mut stream).await;
        assert_eq!(file_status.opcode, u8::from(PeerOpcode::SetRequestedFileId));
        write_frame(
            &mut stream,
            &build_file_status_answer(file_hash, &[true]).expect("status"),
        )
        .await;

        let hashset = read_frame(&mut stream).await;
        assert_eq!(hashset.opcode, u8::from(PeerOpcode::HashsetRequest));
        write_frame(
            &mut stream,
            &build_hashset_answer(file_hash, &[id(0x77)]).expect("hashset"),
        )
        .await;

        let start_upload = read_frame(&mut stream).await;
        assert_eq!(
            start_upload.opcode,
            u8::from(PeerOpcode::StartUploadRequest)
        );
        write_frame(
            &mut stream,
            &peer_frame(PeerOpcode::AcceptUploadRequest, Vec::new()),
        )
        .await;

        let part_request = read_frame(&mut stream).await;
        let ranges = parse_part_request(&part_request.payload, file_hash, false).expect("ranges");
        assert_eq!(ranges, vec![PartRange { begin: 0, end: 4 }]);
        write_frame(
            &mut stream,
            &sending_part(file_hash, PartRange { begin: 0, end: 4 }, b"data"),
        )
        .await;
    });

    let mut runtime = Ed2kPeerRuntime::new(Ed2kPeerRuntimeConfig {
        local_identity: local_identity(),
        io_timeout: Duration::from_secs(1),
        max_packet_size: MAX_PACKET,
        ..Default::default()
    });
    let report = runtime
        .download_once(download_request(addr, file_hash))
        .await
        .expect("download");

    server_task.await.expect("server task");
    assert_eq!(report.phase, PeerRequestPhase::Downloading);
    assert_eq!(report.transfer_status, TransferStatus::Downloading);
    assert_eq!(report.queue_rank, None);
    assert_eq!(report.received_parts.len(), 1);
    assert_eq!(report.received_parts[0].data, b"data");
    assert_eq!(report.sources.len(), 1);
    assert_eq!(report.sources[0].endpoint.ip, 0x0403_0201);
}

#[tokio::test]
async fn peer_runtime_records_queue_rank_without_requesting_parts() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("listener addr");
    let file_hash = id(0x56);
    let server_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_peer_hello_answer(&remote_identity()).expect("hello answer"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_emule_info(remote_identity().udp_port, true).expect("info answer"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_source_exchange_answer(file_hash, 4, true, &[]).expect("source answer"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_file_status_answer(file_hash, &[true]).expect("status"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_hashset_answer(file_hash, &[id(0x78)]).expect("hashset"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &peer_frame(PeerOpcode::QueueRank, 42_u16.to_le_bytes().to_vec()),
        )
        .await;
    });

    let mut runtime = Ed2kPeerRuntime::new(Ed2kPeerRuntimeConfig {
        local_identity: local_identity(),
        io_timeout: Duration::from_secs(1),
        max_packet_size: MAX_PACKET,
        ..Default::default()
    });
    let report = runtime
        .download_once(download_request(addr, file_hash))
        .await
        .expect("download");

    server_task.await.expect("server task");
    assert_eq!(report.phase, PeerRequestPhase::OnQueue);
    assert_eq!(report.queue_rank, Some(42));
    assert!(report.received_parts.is_empty());
}

#[tokio::test]
async fn peer_runtime_rejects_corrupt_part_without_task_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("listener addr");
    let file_hash = id(0x57);
    let server_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_peer_hello_answer(&remote_identity()).expect("hello answer"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_emule_info(remote_identity().udp_port, true).expect("info answer"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_source_exchange_answer(file_hash, 3, false, &[]).expect("source answer"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_file_status_answer(file_hash, &[true]).expect("status"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_hashset_answer(file_hash, &[id(0x79)]).expect("hashset"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &peer_frame(PeerOpcode::AcceptUploadRequest, Vec::new()),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &sending_part(id(0xee), PartRange { begin: 0, end: 3 }, b"bad"),
        )
        .await;
    });

    let mut runtime = Ed2kPeerRuntime::new(Ed2kPeerRuntimeConfig {
        local_identity: local_identity(),
        io_timeout: Duration::from_secs(1),
        max_packet_size: MAX_PACKET,
        ..Default::default()
    });
    let report = runtime
        .download_once(download_request(addr, file_hash))
        .await
        .expect("peer failure should be reported");

    server_task.await.expect("server task");
    assert_eq!(report.phase, PeerRequestPhase::Failed);
    assert_eq!(report.received_parts.len(), 0);
    assert!(matches!(
        report.transfer_status,
        TransferStatus::BackingOff { retry_at: 105 }
    ));
}

#[tokio::test]
async fn peer_runtime_times_out_stalled_part_request_into_backoff() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("listener addr");
    let file_hash = id(0x58);
    let server_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_peer_hello_answer(&remote_identity()).expect("hello answer"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_emule_info(remote_identity().udp_port, true).expect("info answer"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_source_exchange_answer(file_hash, 3, false, &[]).expect("source answer"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_file_status_answer(file_hash, &[true]).expect("status"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &build_hashset_answer(file_hash, &[id(0x80)]).expect("hashset"),
        )
        .await;
        read_frame(&mut stream).await;
        write_frame(
            &mut stream,
            &peer_frame(PeerOpcode::AcceptUploadRequest, Vec::new()),
        )
        .await;
        read_frame(&mut stream).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    });

    let mut runtime = Ed2kPeerRuntime::new(Ed2kPeerRuntimeConfig {
        local_identity: local_identity(),
        io_timeout: Duration::from_millis(10),
        max_packet_size: MAX_PACKET,
        ..Default::default()
    });
    let report = runtime
        .download_once(download_request(addr, file_hash))
        .await
        .expect("stalled peer should be reported");

    server_task.await.expect("server task");
    assert_eq!(report.phase, PeerRequestPhase::Failed);
    assert!(matches!(
        report.transfer_status,
        TransferStatus::BackingOff { retry_at: 105 }
    ));
}

fn local_identity() -> PeerIdentity {
    PeerIdentity {
        user_hash: id(0x11),
        client_id: 0x0102_0304,
        tcp_port: 4662,
        udp_port: 4672,
        kad_udp_port: 4672,
        server: None,
        name: "raria-test".to_string(),
    }
}

fn remote_identity() -> PeerIdentity {
    PeerIdentity {
        user_hash: id(0x22),
        client_id: 0x0506_0708,
        tcp_port: 4662,
        udp_port: 4672,
        kad_udp_port: 4672,
        server: None,
        name: "remote-test".to_string(),
    }
}

fn download_request(addr: SocketAddr, file_hash: [u8; 16]) -> Ed2kPeerDownloadRequest {
    Ed2kPeerDownloadRequest {
        endpoint_host: addr.ip().to_string(),
        endpoint_port: addr.port(),
        file_hash,
        file_size: 4,
        local_part_status: vec![false],
        completed_ranges: Vec::new(),
        globally_requested: Vec::new(),
        hashset_required: true,
        max_new_ranges: 1,
        request_source_exchange: true,
        now_seconds: 100,
    }
}

async fn read_frame(stream: &mut TcpStream) -> PacketFrame {
    let mut header = [0_u8; 6];
    stream.read_exact(&mut header).await.expect("header");
    let len = u32::from_le_bytes(header[1..5].try_into().expect("len")) as usize;
    let mut payload = vec![0_u8; len - 1];
    stream.read_exact(&mut payload).await.expect("payload");
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&payload);
    decode_tcp_frame(&bytes, MAX_PACKET).expect("frame")
}

async fn write_frame(stream: &mut TcpStream, frame: &PacketFrame) {
    let bytes = encode_tcp_frame(frame, MAX_PACKET).expect("encode");
    stream.write_all(&bytes).await.expect("write");
}

fn peer_frame(opcode: PeerOpcode, payload: Vec<u8>) -> PacketFrame {
    PacketFrame {
        protocol: Protocol::Edonkey,
        opcode: opcode.into(),
        payload,
    }
}

fn sending_part(file_hash: [u8; 16], range: PartRange, data: &[u8]) -> PacketFrame {
    let mut payload = file_hash.to_vec();
    payload.extend_from_slice(&(range.begin as u32).to_le_bytes());
    payload.extend_from_slice(&(range.end as u32).to_le_bytes());
    payload.extend_from_slice(data);
    peer_frame(PeerOpcode::SendingPart, payload)
}
