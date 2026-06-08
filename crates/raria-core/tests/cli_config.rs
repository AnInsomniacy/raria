use raria_core::{
    CliCommand, InputTask, OptionDisposition, parse_cli, parse_config_text, parse_input_file_text,
    save_session_text,
};

#[test]
fn parses_common_new_session_cli_options() {
    let command = parse_cli([
        "raria",
        "--dir=/tmp/downloads",
        "--enable-rpc=true",
        "--rpc-listen-port=6800",
        "--rpc-secret=secret",
        "--continue=true",
        "--split=4",
        "--input-file=tasks.txt",
        "--save-session=session.txt",
        "https://example.test/file.iso",
    ])
    .expect("common options should parse");

    assert_eq!(
        command,
        CliCommand {
            dir: Some("/tmp/downloads".into()),
            enable_rpc: true,
            rpc_listen_port: 6800,
            rpc_secret: Some("secret".into()),
            continue_download: true,
            split: Some(4),
            input_file: Some("tasks.txt".into()),
            save_session: Some("session.txt".into()),
            uris: vec!["https://example.test/file.iso".into()],
            dispositions: Vec::new(),
        }
    );
}

#[test]
fn marks_pruned_and_unsupported_options_explicitly() {
    let command = parse_cli([
        "raria",
        "--enable-http-pipelining=true",
        "--ed2k-server=127.0.0.1:4661",
        "https://example.test/file.iso",
    ])
    .expect("known non-kept options should be explicit");

    assert_eq!(
        command.dispositions,
        vec![
            OptionDisposition::Pruned {
                name: "enable-http-pipelining".into()
            },
            OptionDisposition::UnsupportedPhaseOne {
                name: "ed2k-server".into()
            }
        ]
    );
}

#[test]
fn parses_aria_style_config_text() {
    let command = parse_config_text(
        r#"
dir=/var/downloads
enable-rpc=true
rpc-listen-port=6801
rpc-secret=config-secret
continue=true
split=8
"#,
    )
    .expect("config should parse");

    assert_eq!(command.dir.as_deref(), Some("/var/downloads"));
    assert!(command.enable_rpc);
    assert_eq!(command.rpc_listen_port, 6801);
    assert_eq!(command.rpc_secret.as_deref(), Some("config-secret"));
    assert!(command.continue_download);
    assert_eq!(command.split, Some(8));
}

#[test]
fn parses_input_file_tasks_with_per_task_options() {
    let tasks = parse_input_file_text(
        r#"
https://example.test/a.iso
  out=a.iso
  split=2

https://mirror.test/b.iso
https://backup.test/b.iso
  dir=/var/downloads
"#,
    )
    .expect("input file should parse");

    assert_eq!(
        tasks,
        vec![
            InputTask {
                uris: vec!["https://example.test/a.iso".into()],
                options: vec![("out".into(), "a.iso".into()), ("split".into(), "2".into())],
            },
            InputTask {
                uris: vec![
                    "https://mirror.test/b.iso".into(),
                    "https://backup.test/b.iso".into()
                ],
                options: vec![("dir".into(), "/var/downloads".into())],
            },
        ]
    );
}

#[test]
fn saves_new_task_session_as_loadable_input_text() {
    let tasks = vec![InputTask {
        uris: vec!["https://example.test/file.iso".into()],
        options: vec![("dir".into(), "/tmp/downloads".into())],
    }];

    let text = save_session_text(&tasks);
    let loaded = parse_input_file_text(&text).expect("saved session should load");

    assert_eq!(loaded, tasks);
}
