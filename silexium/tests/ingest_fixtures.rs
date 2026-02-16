use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir() -> PathBuf {
    let mut dir = env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    dir.push(format!("silexium-fixtures-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_cmd(mut cmd: Command) {
    let output = cmd.output().expect("failed to run command");
    if !output.status.success() {
        panic!(
            "command failed: {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn ingest_fixture_release() {
    let bin = env!("CARGO_BIN_EXE_silexium");
    let base = unique_temp_dir();
    let xdg = base.join("xdg");
    fs::create_dir_all(&xdg).unwrap();
    let key_dir = base.join("keys");
    fs::create_dir_all(&key_dir).unwrap();

    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let release = fixtures.join("release.toml");
    assert!(release.exists(), "missing fixture release.toml");

    let keys = [
        ("author", "1111111111111111111111111111111111111111111111111111111111111111"),
        ("tests", "2222222222222222222222222222222222222222222222222222222222222222"),
        ("server", "3333333333333333333333333333333333333333333333333333333333333333"),
    ];

    for (role, key_id) in keys {
        let key_path = key_dir.join(format!("{role}.pub"));
        fs::write(&key_path, vec![0u8; 32]).unwrap();
        let mut cmd = Command::new(bin);
        cmd.env("XDG_DATA_HOME", &xdg).args([
            "key",
            "add",
            "--role",
            role,
            "--key",
            key_path.to_str().unwrap(),
            "--key-id",
            key_id,
            "--expires-at",
            "2099-01-01T00:00:00Z",
        ]);
        run_cmd(cmd);
    }

    let mut cmd = Command::new(bin);
    cmd.env("XDG_DATA_HOME", &xdg)
        .env("SILEXIUM_SKIP_PROOF_VERIFY", "1")
        .args(["ingest", "--file", release.to_str().unwrap()]);
    run_cmd(cmd);
}
