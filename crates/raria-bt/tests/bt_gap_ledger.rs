// BitTorrent gap tests.
//
// These tests document BitTorrent parity stop-lines against aria2 1.37.0.
//
// Open gaps stay indexed here with `#[ignore]` markers so the parity scripts can
// map them into `.omx/parity/generated/bt-gap-capability-index.yaml`.
// Once a gap is closed, replace the placeholder with an executable test and
// remove the ignore marker so the generated index can go empty again.

#[cfg(test)]
mod tests {
    use anyhow::{Context, Result};
    use librqbit::{
        AddTorrent, AddTorrentOptions, CreateTorrentOptions, PeerConnectionOptions, Session,
        SessionOptions, create_torrent,
    };
    use russh::keys::ssh_key::rand_core::OsRng;
    use russh::server::{Auth, Msg, Server as _, Session as RusshSession};
    use russh::{Channel, ChannelId};
    use russh_sftp::protocol::{
        Attrs, Data, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
    };
    use raria_bt::service::{
        BtService, BtServiceConfig, BtSource, PeerEncryptionMinLevel, PeerEncryptionMode,
        PeerEncryptionPolicy,
    };
    use raria_core::job::Gid;
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener as StdTcpListener};
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Mutex;
    use tokio::time::{sleep, timeout};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    fn reserve_port() -> u16 {
        let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("ephemeral port addr").port();
        drop(listener);
        port
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn bt_mse_pse_encryption() -> Result<()> {
        let payload: Vec<u8> = (0..2 * 1024 * 1024).map(|idx| ((idx * 29) % 251) as u8).collect();
        let source_root = tempdir().context("seed source tempdir")?;
        let session_root = tempdir().context("seed session tempdir")?;
        let file_name = "encrypted-fixture.bin";
        let source_path = source_root.path().join(file_name);
        std::fs::write(&source_path, &payload).context("write encrypted seed payload")?;

        let torrent = create_torrent(
            &source_path,
            CreateTorrentOptions {
                piece_length: Some(16 * 1024),
                ..Default::default()
            },
        )
        .await
        .context("create encrypted torrent")?;
        let torrent_bytes = torrent
            .as_bytes()
            .context("serialize encrypted torrent")?
            .to_vec();

        let listen_port = reserve_port();
        let seed_session = Session::new_with_opts(
            session_root.path().to_path_buf(),
            SessionOptions {
                disable_dht: true,
                disable_dht_persistence: true,
                listen_port_range: Some(listen_port..(listen_port + 1)),
                enable_upnp_port_forwarding: false,
                peer_opts: Some(PeerConnectionOptions {
                    encryption_policy: Some(PeerEncryptionPolicy {
                        mode: PeerEncryptionMode::Require,
                        min_crypto_level: PeerEncryptionMinLevel::Arc4,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .context("create encrypted seed session")?;

        seed_session
            .add_torrent(
                AddTorrent::from_bytes(torrent.as_bytes().context("seed torrent bytes")?),
                Some(AddTorrentOptions {
                    paused: false,
                    output_folder: Some(source_root.path().to_string_lossy().into_owned()),
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await
            .context("add encrypted seed torrent")?
            .into_handle()
            .context("encrypted seed handle")?
            .wait_until_completed()
            .await
            .context("wait for encrypted seed completion")?;

        let seed_addr = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            seed_session
                .tcp_listen_port()
                .context("encrypted seed listen port")?,
        );

        let download_root = tempdir().context("encrypted download tempdir")?;
        let service = BtService::with_config(
            download_root.path().to_path_buf(),
            BtServiceConfig {
                disable_dht: true,
                disable_dht_persistence: true,
                initial_peers: Some(vec![seed_addr]),
                peer_encryption_policy: PeerEncryptionPolicy {
                    mode: PeerEncryptionMode::Require,
                    min_crypto_level: PeerEncryptionMinLevel::Arc4,
                },
                ..Default::default()
            },
        )
        .context("create encrypted bt service")?;

        let handle = service
            .add(
                BtSource::TorrentBytes(torrent_bytes),
                Gid::from_raw(901),
                None,
                None,
                None,
            )
            .await
            .context("add encrypted torrent to BtService")?;

        timeout(Duration::from_secs(60), async {
            loop {
                let status = service.status(&handle).await.context("encrypted status")?;
                if status.is_complete {
                    return Ok::<_, anyhow::Error>(());
                }
                sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .context("encrypted BT completion timeout")??;

        let out = std::fs::read(download_root.path().join(file_name))
            .context("read encrypted download output")?;
        assert_eq!(out, payload, "encrypted BT payload must match source");

        service.shutdown().await;
        seed_session.stop().await;
        Ok(())
    }

    struct RangeResponder {
        data: Arc<Vec<u8>>,
    }

    const FTP_TEST_USER: &str = "test-user";
    const FTP_TEST_PASSWORD: &str = "test-pass";

    impl RangeResponder {
        fn new(data: Arc<Vec<u8>>) -> Self {
            Self { data }
        }
    }

    async fn create_torrent_bytes(payload: &[u8]) -> Result<Vec<u8>> {
        let source_root = tempdir().context("source tempdir")?;
        let source_path = source_root.path().join("payload.bin");
        std::fs::write(&source_path, payload).context("write source payload")?;
        let torrent = librqbit::create_torrent(&source_path, Default::default())
            .await
            .context("create torrent")?;
        Ok(torrent
            .as_bytes()
            .context("serialize torrent")?
            .to_vec())
    }

    async fn run_bt_webseed_download(
        torrent_bytes: &[u8],
        payload: &[u8],
        web_seed_uris: Vec<String>,
        gid_raw: u64,
    ) -> Result<()> {
        let download_dir = tempdir().context("download tempdir")?;
        let service = BtService::with_config(
            download_dir.path().to_path_buf(),
            BtServiceConfig {
                disable_dht: true,
                disable_dht_persistence: true,
                ..Default::default()
            },
        )
        .context("create bt service")?;

        let handle = service
            .add(
                BtSource::TorrentBytes(torrent_bytes.to_vec()),
                Gid::from_raw(gid_raw),
                None,
                None,
                Some(web_seed_uris),
            )
            .await
            .context("add torrent")?;

        timeout(Duration::from_secs(45), async {
            loop {
                let status = service.status(&handle).await.context("status")?;
                if status.is_complete {
                    return Ok::<_, anyhow::Error>(());
                }
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .context("webseed completion timeout")??;

        let files = service.file_list(&handle).await.context("file list")?;
        assert_eq!(files.len(), 1, "fixture should be single-file");
        let out_path = download_dir.path().join(&files[0].path);
        let out = std::fs::read(&out_path).context("read output file")?;
        assert_eq!(out, payload, "downloaded file must match payload");

        service.shutdown().await;
        Ok(())
    }

    fn parse_range_header(header: &str) -> Option<(usize, usize)> {
        // Expected: bytes=start-end (inclusive).
        let header = header.trim();
        let value = header.strip_prefix("bytes=")?;
        let (start, end) = value.split_once('-')?;
        let start = start.parse::<u64>().ok()?;
        let end = end.parse::<u64>().ok()?;
        if end < start {
            return None;
        }
        Some((start as usize, end as usize))
    }

    impl Respond for RangeResponder {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let total_len = self.data.len();

            let Some(range) = request
                .headers
                .get("range")
                .and_then(|v| v.to_str().ok())
                .and_then(parse_range_header)
            else {
                return ResponseTemplate::new(416);
            };

            let (start, end) = range;
            if start >= total_len || end >= total_len {
                return ResponseTemplate::new(416);
            }

            let body = self.data[start..=end].to_vec();
            ResponseTemplate::new(206)
                .insert_header("accept-ranges", "bytes")
                .insert_header("content-range", format!("bytes {start}-{end}/{total_len}"))
                .set_body_bytes(body)
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bt_webseed_bep17_bep19() -> Result<()> {
        // This contract validates that raria's BT backend can complete a single-file torrent using
        // HTTP WebSeed-style ranged requests, even with no BitTorrent peers available.

        let payload: Arc<Vec<u8>> = Arc::new((0..256 * 1024).map(|i| (i % 251) as u8).collect());
        let torrent_bytes = create_torrent_bytes(payload.as_ref()).await?;

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/payload.bin"))
            .respond_with(RangeResponder::new(payload.clone()))
            .mount(&server)
            .await;

        let webseed_url = format!("{}/payload.bin", server.uri());
        run_bt_webseed_download(&torrent_bytes, payload.as_ref(), vec![webseed_url], 902).await?;
        Ok(())
    }

    #[test]
    fn bt_rarest_first_piece_selection_contract() {
        let ordered = librqbit::parity_contract_sort_piece_candidates(
            librqbit::PieceSelectionStrategy::RarestFirst,
            &[(0, 5), (1, 1), (2, 3)],
        );
        assert_eq!(
            ordered,
            vec![1, 2, 0],
            "rarest-first must prioritize lower-availability pieces"
        );
    }

    struct FtpFixture {
        url: String,
    }

    fn is_disconnect(error: &std::io::Error) -> bool {
        matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::ConnectionAborted
        )
    }

    async fn spawn_ftp_server(file_path: String, file_data: Arc<Vec<u8>>) -> Result<FtpFixture> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind ftp listener")?;
        let port = listener.local_addr().context("control addr")?.port();
        let url = format!(
            "ftp://{FTP_TEST_USER}:{FTP_TEST_PASSWORD}@127.0.0.1:{port}{file_path}",
        );

        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let file_path = file_path.clone();
                let file_data = Arc::clone(&file_data);
                tokio::spawn(async move {
                    handle_ftp_client(stream, &file_path, &file_data).await;
                });
            }
        });

        Ok(FtpFixture { url })
    }

    async fn write_ftp_reply(
        stream: &mut TcpStream,
        code: u16,
        message: &str,
    ) -> std::io::Result<()> {
        stream
            .write_all(format!("{code} {message}\r\n").as_bytes())
            .await?;
        stream.flush().await
    }

    async fn handle_ftp_client(stream: TcpStream, file_path: &str, file_data: &[u8]) {
        let mut reader = BufReader::new(stream);
        let mut pending_offset = 0usize;
        let mut data_listener: Option<TcpListener> = None;
        let mut authenticated = false;
        let mut seen_user: Option<String> = None;

        if write_ftp_reply(reader.get_mut(), 220, "raria ftp test server")
            .await
            .is_err()
        {
            return;
        }

        loop {
            let mut line = String::new();
            let n = match reader.read_line(&mut line).await {
                Ok(n) => n,
                Err(error) if is_disconnect(&error) => break,
                Err(_) => break,
            };
            if n == 0 {
                break;
            }

            let line = line.trim_end_matches(['\r', '\n']);
            let (command, arg) = line
                .split_once(' ')
                .map(|(cmd, rest)| (cmd.to_ascii_uppercase(), rest))
                .unwrap_or_else(|| (line.to_ascii_uppercase(), ""));

            let result = match command.as_str() {
                "USER" => {
                    seen_user = Some(arg.to_string());
                    write_ftp_reply(reader.get_mut(), 331, "password required").await
                }
                "PASS" => {
                    authenticated =
                        seen_user.as_deref() == Some(FTP_TEST_USER) && arg == FTP_TEST_PASSWORD;
                    let code = if authenticated { 230 } else { 530 };
                    let message = if authenticated {
                        "login successful"
                    } else {
                        "login incorrect"
                    };
                    write_ftp_reply(reader.get_mut(), code, message).await
                }
                "TYPE" => write_ftp_reply(reader.get_mut(), 200, "type set to I").await,
                "SIZE" => {
                    if !authenticated || arg != file_path {
                        write_ftp_reply(reader.get_mut(), 550, "not found").await
                    } else {
                        write_ftp_reply(reader.get_mut(), 213, &file_data.len().to_string()).await
                    }
                }
                "REST" => {
                    pending_offset = arg.parse::<usize>().unwrap_or(0);
                    write_ftp_reply(reader.get_mut(), 350, "restart position accepted").await
                }
                "PASV" => {
                    let listener = match TcpListener::bind("127.0.0.1:0").await {
                        Ok(listener) => listener,
                        Err(_) => break,
                    };
                    let addr = match listener.local_addr() {
                        Ok(addr) => addr,
                        Err(_) => break,
                    };
                    let octets = match addr.ip() {
                        std::net::IpAddr::V4(ip) => ip.octets(),
                        std::net::IpAddr::V6(_) => break,
                    };
                    let port_hi = addr.port() / 256;
                    let port_lo = addr.port() % 256;
                    let reply = format!(
                        "Entering Passive Mode ({},{},{},{},{},{})",
                        octets[0], octets[1], octets[2], octets[3], port_hi, port_lo
                    );
                    data_listener = Some(listener);
                    write_ftp_reply(reader.get_mut(), 227, &reply).await
                }
                "RETR" => {
                    if !authenticated || arg != file_path {
                        write_ftp_reply(reader.get_mut(), 550, "not found").await
                    } else {
                        let Some(listener) = data_listener.take() else {
                            let _ = write_ftp_reply(reader.get_mut(), 425, "no data channel").await;
                            continue;
                        };
                        if write_ftp_reply(reader.get_mut(), 150, "opening data connection")
                            .await
                            .is_err()
                        {
                            break;
                        }
                        let Ok((mut data_stream, _)) = listener.accept().await else {
                            break;
                        };
                        let start = pending_offset.min(file_data.len());
                        if data_stream.write_all(&file_data[start..]).await.is_err() {
                            break;
                        }
                        let _ = data_stream.shutdown().await;
                        pending_offset = 0;
                        write_ftp_reply(reader.get_mut(), 226, "transfer complete").await
                    }
                }
                "QUIT" => {
                    let _ = write_ftp_reply(reader.get_mut(), 221, "goodbye").await;
                    break;
                }
                "PBSZ" => write_ftp_reply(reader.get_mut(), 200, "pbsz=0").await,
                "PROT" => write_ftp_reply(reader.get_mut(), 200, "protection level set").await,
                _ => write_ftp_reply(reader.get_mut(), 502, "not implemented").await,
            };

            if result.is_err() {
                break;
            }
        }
    }

    #[derive(Clone)]
    struct TestSftpServer {
        file_path: String,
        file_data: Arc<Vec<u8>>,
    }

    impl russh::server::Server for TestSftpServer {
        type Handler = SshSession;

        fn new_client(&mut self, _: Option<SocketAddr>) -> Self::Handler {
            SshSession {
                file_path: self.file_path.clone(),
                file_data: Arc::clone(&self.file_data),
                clients: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    struct SshSession {
        file_path: String,
        file_data: Arc<Vec<u8>>,
        clients: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    }

    impl SshSession {
        async fn get_channel(&mut self, channel_id: ChannelId) -> Channel<Msg> {
            let mut clients = self.clients.lock().await;
            clients.remove(&channel_id).expect("channel must exist")
        }
    }

    impl russh::server::Handler for SshSession {
        type Error = anyhow::Error;

        async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
            if user == FTP_TEST_USER && password == FTP_TEST_PASSWORD {
                Ok(Auth::Accept)
            } else {
                Ok(Auth::reject())
            }
        }

        async fn auth_publickey(
            &mut self,
            user: &str,
            _public_key: &russh::keys::PublicKey,
        ) -> Result<Auth, Self::Error> {
            if user == FTP_TEST_USER {
                Ok(Auth::Accept)
            } else {
                Ok(Auth::reject())
            }
        }

        async fn channel_open_session(
            &mut self,
            channel: Channel<Msg>,
            _session: &mut RusshSession,
        ) -> Result<bool, Self::Error> {
            let mut clients = self.clients.lock().await;
            clients.insert(channel.id(), channel);
            Ok(true)
        }

        async fn subsystem_request(
            &mut self,
            channel_id: ChannelId,
            name: &str,
            session: &mut RusshSession,
        ) -> Result<(), Self::Error> {
            if name == "sftp" {
                let channel = self.get_channel(channel_id).await;
                let sftp = SftpSession {
                    file_path: self.file_path.clone(),
                    file_data: Arc::clone(&self.file_data),
                };
                session.channel_success(channel_id)?;
                russh_sftp::server::run(channel.into_stream(), sftp).await;
            } else {
                session.channel_failure(channel_id)?;
            }
            Ok(())
        }
    }

    struct SftpSession {
        file_path: String,
        file_data: Arc<Vec<u8>>,
    }

    impl russh_sftp::server::Handler for SftpSession {
        type Error = StatusCode;

        fn unimplemented(&self) -> Self::Error {
            StatusCode::OpUnsupported
        }

        async fn init(
            &mut self,
            _version: u32,
            _extensions: HashMap<String, String>,
        ) -> Result<Version, Self::Error> {
            Ok(Version::new())
        }

        async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
            let resolved = if path == "." { "/".to_string() } else { path };
            Ok(Name {
                id,
                files: vec![russh_sftp::protocol::File::dummy(resolved)],
            })
        }

        async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
            if path != self.file_path {
                return Err(StatusCode::NoSuchFile);
            }
            let mut attrs = FileAttributes::empty();
            attrs.size = Some(self.file_data.len() as u64);
            attrs.permissions = Some(0o644);
            Ok(Attrs { id, attrs })
        }

        async fn open(
            &mut self,
            id: u32,
            filename: String,
            _pflags: OpenFlags,
            _attrs: FileAttributes,
        ) -> Result<Handle, Self::Error> {
            if filename != self.file_path {
                return Err(StatusCode::NoSuchFile);
            }
            Ok(Handle {
                id,
                handle: filename,
            })
        }

        async fn read(
            &mut self,
            id: u32,
            handle: String,
            offset: u64,
            len: u32,
        ) -> Result<Data, Self::Error> {
            if handle != self.file_path {
                return Err(StatusCode::NoSuchFile);
            }
            let start = offset as usize;
            if start >= self.file_data.len() {
                return Err(StatusCode::Eof);
            }
            let end = (start + len as usize).min(self.file_data.len());
            Ok(Data {
                id,
                data: self.file_data[start..end].to_vec(),
            })
        }

        async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
            Ok(Status {
                id,
                status_code: StatusCode::Ok,
                error_message: "ok".into(),
                language_tag: "en-US".into(),
            })
        }
    }

    struct SftpFixture {
        url: String,
    }

    async fn spawn_sftp_server(file_path: String, file_data: Arc<Vec<u8>>) -> Result<SftpFixture> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").context("bind sftp port")?;
        let port = listener.local_addr().context("sftp local addr")?.port();
        drop(listener);

        let host_key = russh::keys::PrivateKey::random(
            &mut OsRng,
            russh::keys::ssh_key::Algorithm::Ed25519,
        )
        .context("generate ssh host key")?;

        let config = russh::server::Config {
            auth_rejection_time: Duration::from_secs(1),
            auth_rejection_time_initial: Some(Duration::from_secs(0)),
            keys: vec![host_key],
            ..Default::default()
        };

        let mut server = TestSftpServer {
            file_path: file_path.clone(),
            file_data,
        };

        tokio::spawn(async move {
            let _ = server
                .run_on_address(Arc::new(config), ("127.0.0.1", port))
                .await;
        });

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                break;
            }
            if std::time::Instant::now() >= deadline {
                anyhow::bail!("sftp test server did not become ready in time");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        Ok(SftpFixture {
            url: format!("sftp://{FTP_TEST_USER}:{FTP_TEST_PASSWORD}@127.0.0.1:{port}{file_path}"),
        })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn bt_mixed_protocol_source_download() -> Result<()> {
        // Contract: the BT runtime can consume FTP and SFTP auxiliary ranged sources
        // as piece providers for the same torrent payload.
        let payload: Arc<Vec<u8>> = Arc::new((0..256 * 1024).map(|i| (i % 251) as u8).collect());
        let torrent_bytes = create_torrent_bytes(payload.as_ref()).await?;
        let file_path = "/payload.bin".to_string();

        let ftp_fixture = spawn_ftp_server(file_path.clone(), Arc::clone(&payload)).await?;
        run_bt_webseed_download(&torrent_bytes, payload.as_ref(), vec![ftp_fixture.url.clone()], 903)
            .await?;

        let sftp_fixture = spawn_sftp_server(file_path, Arc::clone(&payload)).await?;
        run_bt_webseed_download(
            &torrent_bytes,
            payload.as_ref(),
            vec![sftp_fixture.url.clone()],
            904,
        )
        .await?;

        // Mixed auxiliary source list remains accepted and completes.
        run_bt_webseed_download(
            &torrent_bytes,
            payload.as_ref(),
            vec![ftp_fixture.url, sftp_fixture.url],
            905,
        )
        .await?;
        Ok(())
    }
}
