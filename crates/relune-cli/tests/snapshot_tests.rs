//! Snapshot tests for CLI help output.
//!
//! These tests capture the help output for each command to ensure
//! the CLI interface remains stable and documented.

use assert_cmd::Command;

fn relune() -> Command {
    Command::cargo_bin("relune").expect("Failed to find relune binary")
}

#[test]
fn snapshot_help_root() {
    let mut cmd = relune();
    let output = cmd.arg("--help").assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    insta::assert_snapshot!("help_root", stdout);
}

#[test]
fn snapshot_help_render() {
    let mut cmd = relune();
    let output = cmd.arg("render").arg("--help").assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    insta::assert_snapshot!("help_render", stdout);
}

#[test]
fn snapshot_help_inspect() {
    let mut cmd = relune();
    let output = cmd.arg("inspect").arg("--help").assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    insta::assert_snapshot!("help_inspect", stdout);
}

#[test]
fn snapshot_help_export() {
    let mut cmd = relune();
    let output = cmd.arg("export").arg("--help").assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    insta::assert_snapshot!("help_export", stdout);
}

#[test]
fn snapshot_help_diff() {
    let mut cmd = relune();
    let output = cmd.arg("diff").arg("--help").assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    insta::assert_snapshot!("help_diff", stdout);
}

#[test]
fn snapshot_version() {
    let mut cmd = relune();
    let output = cmd.arg("--version").assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    insta::assert_snapshot!("version", stdout);
}
