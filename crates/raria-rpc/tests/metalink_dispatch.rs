// Integration tests for Metalink → Job dispatch via RPC.
//
// Verifies that addMetalink parses XML and creates download jobs
// for each file in the metalink document.

#[cfg(test)]
mod tests {
    use raria_core::config::GlobalConfig;
    use raria_core::engine::Engine;
    use raria_rpc::server::{RpcServerConfig, start_rpc_server};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    async fn spawn_server() -> (Arc<Engine>, String, CancellationToken) {
        let config = GlobalConfig {
            dir: std::path::PathBuf::from("/tmp/rpc_test_metalink"),
            ..Default::default()
        };
        let engine = Arc::new(Engine::new(config));
        let cancel = CancellationToken::new();
        let rpc_config = RpcServerConfig {
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
        };
        let addrs = start_rpc_server(engine.clone(), &rpc_config, cancel.clone())
            .await
            .unwrap();
        let url = format!("http://{}", addrs.rpc);
        (engine, url, cancel)
    }

    async fn rpc_call(url: &str, method: &str, params: serde_json::Value) -> serde_json::Value {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });
        reqwest::Client::new()
            .post(url)
            .json(&body)
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap()
    }

    /// addMetalink with valid Metalink v3 XML should create jobs for each file.
    #[tokio::test]
    async fn add_metalink_creates_jobs_from_xml() {
        let (engine, url, cancel) = spawn_server().await;

        let metalink_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink version="3.0" xmlns="http://www.metalinker.org/">
  <files>
    <file name="test.zip">
      <size>1048576</size>
      <resources>
        <url type="http" preference="100">https://mirror1.com/test.zip</url>
        <url type="http" preference="90">https://mirror2.com/test.zip</url>
      </resources>
    </file>
  </files>
</metalink>"#;

        // base64-encode the XML (aria2 expects base64 for addMetalink).
        use base64::Engine as Base64Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(metalink_xml);

        let resp = rpc_call(&url, "aria2.addMetalink", serde_json::json!([encoded])).await;

        // Should return an array of GIDs (one per file in the metalink).
        assert!(
            resp.get("error").is_none(),
            "addMetalink should succeed: {resp}"
        );
        let result = resp["result"].as_array().unwrap();
        assert_eq!(result.len(), 1, "one file in metalink = one GID");

        // Verify the job exists in the engine.
        let gid_str = result[0].as_str().unwrap();
        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert!(
            job.uris.iter().any(|u| u.contains("mirror1.com")),
            "job should have mirror1 URL"
        );
        assert_eq!(
            job.uris.first().map(String::as_str),
            Some("https://mirror2.com/test.zip"),
            "normalized URLs should be sorted by Metalink priority/preference"
        );

        cancel.cancel();
    }

    /// addMetalink with multi-file metalink creates multiple jobs.
    #[tokio::test]
    async fn add_metalink_multi_file_creates_multiple_jobs() {
        let (engine, url, cancel) = spawn_server().await;

        let metalink_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink version="3.0" xmlns="http://www.metalinker.org/">
  <files>
    <file name="file1.bin">
      <resources>
        <url type="http">https://cdn.com/file1.bin</url>
      </resources>
    </file>
    <file name="file2.bin">
      <resources>
        <url type="http">https://cdn.com/file2.bin</url>
      </resources>
    </file>
  </files>
</metalink>"#;

        use base64::Engine as Base64Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(metalink_xml);

        let resp = rpc_call(&url, "aria2.addMetalink", serde_json::json!([encoded])).await;

        assert!(resp.get("error").is_none(), "should succeed: {resp}");
        let result = resp["result"].as_array().unwrap();
        assert_eq!(result.len(), 2, "two files = two GIDs");

        // Verify both jobs exist.
        for gid_str in result {
            let gid_str = gid_str.as_str().unwrap();
            let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(gid_str, 16).unwrap());
            assert!(engine.registry.get(gid).is_some());
        }

        cancel.cancel();
    }

    /// addMetalink with invalid base64 should return error.
    #[tokio::test]
    async fn add_metalink_invalid_base64_returns_error() {
        let (_engine, url, cancel) = spawn_server().await;

        let resp = rpc_call(
            &url,
            "aria2.addMetalink",
            serde_json::json!(["not-valid-base64!!!"]),
        )
        .await;

        assert!(
            resp.get("error").is_some(),
            "invalid base64 should error: {resp}"
        );

        cancel.cancel();
    }

    /// addMetalink with invalid XML should return error.
    #[tokio::test]
    async fn add_metalink_invalid_xml_returns_error() {
        let (_engine, url, cancel) = spawn_server().await;

        use base64::Engine as Base64Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode("<not-metalink/>");

        let resp = rpc_call(&url, "aria2.addMetalink", serde_json::json!([encoded])).await;

        assert!(
            resp.get("error").is_some(),
            "invalid XML should error: {resp}"
        );

        cancel.cancel();
    }

    #[tokio::test]
    async fn add_metalink_sets_job_checksum_from_best_hash() {
        let (engine, url, cancel) = spawn_server().await;

        let metalink_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink version="3.0" xmlns="http://www.metalinker.org/">
  <files>
    <file name="test.zip">
      <verification>
        <hash type="md5">D41D8CD98F00B204E9800998ECF8427E</hash>
        <hash type="sha-256">ABCDEF123456</hash>
      </verification>
      <resources>
        <url type="http">https://mirror1.com/test.zip</url>
      </resources>
    </file>
  </files>
</metalink>"#;

        use base64::Engine as Base64Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(metalink_xml);

        let resp = rpc_call(&url, "aria2.addMetalink", serde_json::json!([encoded])).await;

        assert!(
            resp.get("error").is_none(),
            "addMetalink should succeed: {resp}"
        );
        let gid_str = resp["result"].as_array().unwrap()[0].as_str().unwrap();
        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(
            job.options.checksum.as_deref(),
            Some("sha-256=abcdef123456")
        );

        cancel.cancel();
    }

    #[tokio::test]
    async fn add_metalink_preserves_expected_size_and_piece_checksums() {
        let (engine, url, cancel) = spawn_server().await;

        let metalink_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="piece.bin">
    <size>2048</size>
    <pieces type="sha-256" length="1024">
      <hash>AA</hash>
      <hash>BB</hash>
    </pieces>
    <url priority="1">https://mirror1.com/piece.bin</url>
  </file>
</metalink>"#;

        use base64::Engine as Base64Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(metalink_xml);

        let resp = rpc_call(&url, "aria2.addMetalink", serde_json::json!([encoded])).await;
        assert!(
            resp.get("error").is_none(),
            "addMetalink should succeed: {resp}"
        );

        let gid_str = resp["result"].as_array().unwrap()[0].as_str().unwrap();
        let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(gid_str, 16).unwrap());
        let job = engine.registry.get(gid).unwrap();
        assert_eq!(job.total_size, Some(2048));
        let piece_checksum = job.piece_checksum.as_ref().expect("piece checksum");
        assert_eq!(piece_checksum.algo, "sha-256");
        assert_eq!(piece_checksum.length, 1024);
        assert_eq!(piece_checksum.hashes, vec!["aa", "bb"]);

        cancel.cancel();
    }

    #[tokio::test]
    async fn add_metalink_propagates_common_rpc_options_to_each_job() {
        let (engine, url, cancel) = spawn_server().await;

        let output_dir = std::env::temp_dir().join(format!(
            "rpc_test_metalink_opts_{}",
            std::process::id()
        ));

        let metalink_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink version="3.0" xmlns="http://www.metalinker.org/">
  <files>
    <file name="file1.bin">
      <resources>
        <url type="http">https://cdn.com/file1.bin</url>
      </resources>
    </file>
    <file name="file2.bin">
      <resources>
        <url type="http">https://cdn.com/file2.bin</url>
      </resources>
    </file>
  </files>
</metalink>"#;

        use base64::Engine as Base64Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(metalink_xml);

        let resp = rpc_call(
            &url,
            "aria2.addMetalink",
            serde_json::json!([
                encoded,
                {
                    "dir": output_dir,
                    "split": "4",
                    "max-download-limit": "102400",
                    "header": ["X-Metalink-Header: from-rpc"],
                    "http-user": "rpc-user",
                    "http-passwd": "rpc-pass"
                }
            ]),
        )
        .await;

        assert!(resp.get("error").is_none(), "should succeed: {resp}");
        let result = resp["result"].as_array().unwrap();
        assert_eq!(result.len(), 2, "two files = two GIDs");

        for gid_str in result {
            let gid_str = gid_str.as_str().unwrap();
            let gid = raria_core::job::Gid::from_raw(u64::from_str_radix(gid_str, 16).unwrap());
            let job = engine.registry.get(gid).unwrap();
            assert_eq!(job.out_path.parent(), Some(output_dir.as_path()));
            assert_eq!(job.options.max_connections, 4);
            assert_eq!(job.options.max_download_limit, 102400);
            assert_eq!(
                job.options.headers,
                vec![("X-Metalink-Header".into(), "from-rpc".into())]
            );
            assert_eq!(job.options.http_user.as_deref(), Some("rpc-user"));
            assert_eq!(job.options.http_passwd.as_deref(), Some("rpc-pass"));
        }

        cancel.cancel();
    }

    #[tokio::test]
    async fn add_metalink_links_multifile_jobs_with_lightweight_relations() {
        let (engine, url, cancel) = spawn_server().await;

        let metalink_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink version="3.0" xmlns="http://www.metalinker.org/">
  <files>
    <file name="file1.bin">
      <resources>
        <url type="http">https://cdn.com/file1.bin</url>
      </resources>
    </file>
    <file name="file2.bin">
      <resources>
        <url type="http">https://cdn.com/file2.bin</url>
      </resources>
    </file>
    <file name="file3.bin">
      <resources>
        <url type="http">https://cdn.com/file3.bin</url>
      </resources>
    </file>
  </files>
</metalink>"#;

        use base64::Engine as Base64Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(metalink_xml);
        let resp = rpc_call(&url, "aria2.addMetalink", serde_json::json!([encoded])).await;
        assert!(resp.get("error").is_none(), "should succeed: {resp}");

        let gids = resp["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(gids.len(), 3);

        let root = raria_core::job::Gid::from_raw(u64::from_str_radix(&gids[0], 16).unwrap());
        let second = raria_core::job::Gid::from_raw(u64::from_str_radix(&gids[1], 16).unwrap());
        let third = raria_core::job::Gid::from_raw(u64::from_str_radix(&gids[2], 16).unwrap());

        let root_job = engine.registry.get(root).unwrap();
        assert_eq!(root_job.followed_by, vec![second]);
        assert_eq!(root_job.following, None);
        assert_eq!(root_job.belongs_to, None);

        let second_job = engine.registry.get(second).unwrap();
        assert_eq!(second_job.following, Some(root));
        assert_eq!(second_job.followed_by, vec![third]);
        assert_eq!(second_job.belongs_to, Some(root));

        let third_job = engine.registry.get(third).unwrap();
        assert_eq!(third_job.following, Some(second));
        assert!(third_job.followed_by.is_empty());
        assert_eq!(third_job.belongs_to, Some(root));

        cancel.cancel();
    }
}
