use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_help() {
    let mut cmd = Command::cargo_bin("colx").unwrap();
    cmd.arg("--help");
    cmd.assert().success().stdout(predicate::str::contains(
        "Extract the specified columns from FILES or stdin.",
    ));
}

#[test]
fn test_version() {
    let mut cmd = Command::cargo_bin("colx").unwrap();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("colx 1.0.5"));
}

#[test]
fn test_basic_file() {
    let mut cmd = Command::cargo_bin("colx").unwrap();
    cmd.arg("1").arg("testdata/file1");
    cmd.assert().success().stdout("This\n\nIt\n");
}

#[test]
fn test_negative_columns() {
    let mut cmd = Command::cargo_bin("colx").unwrap();
    cmd.arg("--")
        .arg("-1")
        .arg("testdata/file_with_empty_columns");
    cmd.assert().success().stdout("after\n");
}

#[test]
fn test_missing_columns() {
    let mut cmd = Command::cargo_bin("colx").unwrap();
    cmd.arg("testdata/file1");
    cmd.assert().failure().stderr(predicate::str::contains(
        "At least one column or column range must be provided.",
    ));
}

#[test]
fn test_invalid_delimiter() {
    let mut cmd = Command::cargo_bin("colx").unwrap();
    cmd.arg("--delimiter")
        .arg("[as")
        .arg("1")
        .arg("testdata/file1");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Failed compiling delimiter regex"));
}

#[test]
fn test_non_existent_file() {
    let mut cmd = Command::cargo_bin("colx").unwrap();
    cmd.arg("1").arg("testdata/non_existent");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("No such file or directory"));
}

#[test]
fn test_stdin() {
    let mut cmd = Command::cargo_bin("colx").unwrap();
    cmd.arg("1").write_stdin("hello world\nfoo bar\n");
    cmd.assert().success().stdout("hello\nfoo\n");
}

#[test]
fn test_multiple_files() {
    let mut cmd = Command::cargo_bin("colx").unwrap();
    cmd.arg("1").arg("testdata/file1").arg("testdata/file2");
    cmd.assert()
        .success()
        .stdout("This\n\nIt\nFile\n\n\nIt\n\n");
}

#[test]
fn test_column_range() {
    let mut cmd = Command::cargo_bin("colx").unwrap();
    cmd.arg("1:2").arg("testdata/file1");
    cmd.assert().success().stdout("This is\n\nIt is\n");
}

#[test]
fn test_reversed_range() {
    let mut cmd = Command::cargo_bin("colx").unwrap();
    cmd.arg("2:1").arg("testdata/file1");
    cmd.assert().success().stdout("is This\n\nis It\n");
}
