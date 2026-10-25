use std::process::Command;

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_folio-cli"))
        .args(args)
        .output()
        .expect("run folio-cli")
}

#[test]
fn prints_version() {
    let out = cli(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        concat!("folio-cli ", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn rejects_unknown_command() {
    let out = cli(&["frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
}
