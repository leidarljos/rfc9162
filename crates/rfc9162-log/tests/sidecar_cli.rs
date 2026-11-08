//! The sidecar from the command line: init, sync, snapshot, history, tree,
//! diff and blob over a project directory, with a PEM signing key.

use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::SigningKey;
use std::path::Path;
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rfc9162-sidecar"))
        .args(args)
        .output()
        .expect("run rfc9162-sidecar")
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).expect("utf8 stdout")
}

fn write(project: &Path, rel: &str, bytes: &[u8]) {
    let path = project.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn a_project_is_synced_snapshotted_and_read_back_from_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("etc");
    std::fs::create_dir_all(&project).unwrap();
    let p = project.to_str().unwrap();
    write(&project, "hosts", b"127.0.0.1 localhost\n");
    write(&project, "ssh/sshd_config", b"Port 22\n");
    let key = dir.path().join("sidecar.pem");
    std::fs::write(
        &key,
        SigningKey::from_bytes(&[7u8; 32])
            .to_pkcs8_pem(LineEnding::LF)
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    let k = key.to_str().unwrap();

    assert!(ok(&cli(&["init", "--dir", p])).contains("size 0"));
    let first = ok(&cli(&["sync", "--dir", p, "--meta", "actor=root"]));
    assert!(first.contains("added hosts\n"), "{first}");
    assert!(
        first.contains("synced: 2 added, 0 changed, 0 deleted, 0 unchanged; entries 0..2; size 2"),
        "{first}"
    );
    let snap = ok(&cli(&[
        "snapshot",
        "--dir",
        p,
        "--key",
        k,
        "--origin",
        "example.org/etc",
    ]));
    assert!(
        snap.starts_with("snapshot example.org/etc size 3 root "),
        "{snap}"
    );

    write(&project, "ssh/sshd_config", b"Port 2222\n");
    std::fs::remove_file(project.join("hosts")).unwrap();
    let second = ok(&cli(&["sync", "--dir", p]));
    assert!(
        second.contains("changed ssh/sshd_config\n") && second.contains("deleted hosts\n"),
        "{second}"
    );

    let history = ok(&cli(&["history", "--dir", p, "ssh/sshd_config"]));
    let lines: Vec<&str> = history.lines().collect();
    assert_eq!(lines.len(), 2, "{history}");
    assert!(lines[0].starts_with("1\tput\tssh/sshd_config\t"));
    assert!(lines[1].starts_with("3\tput\tssh/sshd_config\t"));
    let blob = lines[1].split('\t').nth(3).unwrap();
    assert_eq!(ok(&cli(&["blob", "--dir", p, blob])), "Port 2222\n");

    let tree = ok(&cli(&["tree", "--dir", p]));
    assert_eq!(tree.lines().count(), 1, "{tree}");
    assert!(tree.starts_with("ssh/sshd_config\t3\t"));
    let tree_at_two = ok(&cli(&["tree", "--dir", p, "2"]));
    assert_eq!(tree_at_two.lines().count(), 2, "{tree_at_two}");
    let diff = ok(&cli(&["diff", "--dir", p, "3", "5"]));
    assert!(
        diff.contains("3\tput\tssh/sshd_config") && diff.contains("4\tdelete\thosts"),
        "{diff}"
    );

    // Only the pack and its lock sit beside the project.
    let sidecar: Vec<_> = std::fs::read_dir(project.join(".sidecar"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(sidecar, vec!["pack".to_string()]);

    // A command line this binary does not take is status 2.
    assert_eq!(cli(&["tree"]).status.code(), Some(2));
    assert_eq!(cli(&["frobnicate", "--dir", p]).status.code(), Some(2));
    assert_eq!(
        cli(&["blob", "--dir", p, &"0".repeat(64)]).status.code(),
        Some(1)
    );
}
