use std::process::Command;

fn cargo_bin(name: &str) -> String {
    std::env::var(format!("CARGO_BIN_EXE_{name}")).expect("cargo should provide binary path")
}

#[test]
fn completion_generates_native_script_without_runtime_logs() {
    let output = Command::new(cargo_bin("raria"))
        .args(["completion", "bash"])
        .output()
        .expect("run completion command");

    assert!(
        output.status.success(),
        "completion command failed: {output:?}"
    );
    assert!(
        output.stderr.is_empty(),
        "completion should not write stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("completion output is UTF-8");
    assert!(stdout.starts_with("_raria()"));
    assert!(stdout.contains("--api-port"));
    assert!(stdout.contains("--download-dir"));

    let removed_strings = [
        "logging initialized",
        &["--", "rpc", "-port"].concat(),
        &["json", "rpc"].concat(),
        &["JSON", "-", "RPC"].concat(),
        &["aria", "2"].concat(),
        &["add", "Uri"].concat(),
        &["tell", "Status"].concat(),
    ];

    for removed in removed_strings {
        assert!(
            !stdout.contains(removed),
            "completion exposed removed surface: {removed}"
        );
    }
}
