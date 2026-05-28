use raria_ed2k::opcode::ServerOpcode;
use raria_ed2k::packet::{
    PacketFrame, Protocol, decode_tcp_frame, decode_udp_datagram, encode_tcp_frame,
    encode_udp_datagram,
};
use raria_ed2k::runtime::{
    Ed2kServerEndpoint, Ed2kServerRuntime, Ed2kServerRuntimeConfig, Ed2kSourceQuery,
};
use raria_ed2k::server::{ServerReachability, ServerStatus};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

const MAX_PACKET: usize = 16 * 1024;

#[tokio::test]
async fn tcp_server_runtime_logs_in_and_collects_sources_from_local_server() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let addr = listener.local_addr().expect("addr");
    let file_hash = [0x42; 16];
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let login = read_tcp_frame(&mut socket).await;
        assert_eq!(login.opcode, u8::from(ServerOpcode::LoginRequest));
        let source_request = read_tcp_frame(&mut socket).await;
        assert_eq!(source_request.opcode, u8::from(ServerOpcode::GetSources));

        let mut id_change = Vec::new();
        id_change.extend_from_slice(&0x0102_0304_u32.to_le_bytes());
        socket
            .write_all(&tcp_frame(ServerOpcode::IdChange, id_change))
            .await
            .expect("idchange");
        socket
            .write_all(&tcp_frame(
                ServerOpcode::ServerStatus,
                [7_u32.to_le_bytes(), 90_u32.to_le_bytes()].concat(),
            ))
            .await
            .expect("status");
        let mut found = Vec::new();
        found.extend_from_slice(&file_hash);
        found.push(1);
        found.extend_from_slice(&0x0506_0708_u32.to_le_bytes());
        found.extend_from_slice(&4662_u16.to_le_bytes());
        socket
            .write_all(&tcp_frame(ServerOpcode::FoundSources, found))
            .await
            .expect("found sources");
    });

    let runtime = Ed2kServerRuntime::new(Ed2kServerRuntimeConfig::default());
    let report = runtime
        .query_tcp_sources(
            Ed2kServerEndpoint::new("127.0.0.1", addr.port(), addr.port()),
            Ed2kSourceQuery {
                file_hash,
                file_size: 700,
            },
        )
        .await
        .expect("tcp report");

    server.await.expect("server task");
    assert_eq!(report.state.reachability, Some(ServerReachability::HighId));
    assert_eq!(
        report.state.status,
        Some(ServerStatus {
            users: 7,
            files: 90
        })
    );
    assert_eq!(report.sources.len(), 1);
    assert_eq!(report.sources[0].client_id, 0x0506_0708);
    assert_eq!(report.sources[0].tcp_port, 4662);
}

#[tokio::test]
async fn udp_server_runtime_collects_status_and_sources_from_local_server() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("udp server");
    let addr = socket.local_addr().expect("addr");
    let file_hash = [0x66; 16];
    let server = tokio::spawn(async move {
        let mut buf = [0_u8; 2048];
        let (len, peer) = socket.recv_from(&mut buf).await.expect("status request");
        let status_request = decode_udp_datagram(&buf[..len], MAX_PACKET).expect("status frame");
        assert_eq!(
            status_request.opcode,
            u8::from(ServerOpcode::GlobalServerStatusRequest)
        );
        let mut status = Vec::new();
        status.extend_from_slice(&0xaabb_ccdd_u32.to_le_bytes());
        status.extend_from_slice(&12_u32.to_le_bytes());
        status.extend_from_slice(&34_u32.to_le_bytes());
        socket
            .send_to(
                &udp_frame(ServerOpcode::GlobalServerStatusResponse, status),
                peer,
            )
            .await
            .expect("send status");

        let (len, peer) = socket.recv_from(&mut buf).await.expect("source request");
        let source_request = decode_udp_datagram(&buf[..len], MAX_PACKET).expect("source frame");
        assert_eq!(
            source_request.opcode,
            u8::from(ServerOpcode::GlobalGetSources2)
        );
        let mut found = Vec::new();
        found.extend_from_slice(&file_hash);
        found.push(1);
        found.extend_from_slice(&0x0102_0304_u32.to_le_bytes());
        found.extend_from_slice(&4663_u16.to_le_bytes());
        socket
            .send_to(&udp_frame(ServerOpcode::GlobalFoundSources, found), peer)
            .await
            .expect("send sources");
    });

    let runtime = Ed2kServerRuntime::new(Ed2kServerRuntimeConfig::default());
    let report = runtime
        .query_udp_sources(
            Ed2kServerEndpoint::new("127.0.0.1", addr.port(), addr.port()),
            Ed2kSourceQuery {
                file_hash,
                file_size: 700,
            },
            0xaabb_ccdd,
        )
        .await
        .expect("udp report");

    server.await.expect("server task");
    let status = report.status.expect("status");
    assert_eq!(status.users, 12);
    assert_eq!(status.files, 34);
    assert_eq!(report.sources.len(), 1);
    assert_eq!(report.sources[0].client_id, 0x0102_0304);
    assert_eq!(report.sources[0].tcp_port, 4663);
}

async fn read_tcp_frame(socket: &mut TcpStream) -> PacketFrame {
    let mut header = [0_u8; 6];
    socket.read_exact(&mut header).await.expect("header");
    let length = u32::from_le_bytes(header[1..5].try_into().unwrap()) as usize;
    let mut payload = vec![0_u8; length - 1];
    socket.read_exact(&mut payload).await.expect("payload");
    let mut raw = header.to_vec();
    raw.extend_from_slice(&payload);
    decode_tcp_frame(&raw, MAX_PACKET).expect("tcp frame")
}

fn tcp_frame(opcode: ServerOpcode, payload: Vec<u8>) -> Vec<u8> {
    encode_tcp_frame(
        &PacketFrame {
            protocol: Protocol::Edonkey,
            opcode: opcode.into(),
            payload,
        },
        MAX_PACKET,
    )
    .expect("encode tcp")
}

fn udp_frame(opcode: ServerOpcode, payload: Vec<u8>) -> Vec<u8> {
    encode_udp_datagram(
        &PacketFrame {
            protocol: Protocol::Edonkey,
            opcode: opcode.into(),
            payload,
        },
        MAX_PACKET,
    )
    .expect("encode udp")
}
