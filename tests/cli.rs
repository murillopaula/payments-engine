use assert_cmd::prelude::*;
use predicates::prelude::*;
use rstest::rstest;
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

// Helper function to create a temporary CSV file
fn create_temp_csv(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "{}", content).unwrap();
    file
}

#[rstest]
fn test_cli_success() {
    let input_content = "type,client,tx,amount\n\
                         deposit,1,1,10.0\n\
                         withdrawal,1,2,5.0";
    let input_file = create_temp_csv(input_content);

    let expected_output = "client,available,held,total,locked\n\
                           1,5.0000,0.0000,5.0000,false";

    let mut cmd = Command::cargo_bin("payment_engine").unwrap();
    cmd.arg(input_file.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::diff(expected_output).trim()) // Compare stdout (trimmed)
        .stderr(predicate::str::is_empty());
}

#[rstest]
fn test_cli_no_args() {
    let mut cmd = Command::cargo_bin("payment_engine").unwrap();
    cmd.assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Usage:"));
}

#[rstest]
fn test_cli_file_not_found() {
    let mut cmd = Command::cargo_bin("payment_engine").unwrap();
    cmd.arg("non_existent_file_12345.csv");
    cmd.assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "Error processing transactions: IO error:",
        ));
}

#[rstest]
fn test_cli_bad_csv_skips_and_warns() {
    let input_content = "type,client,tx,amount\n\
                         deposit,1,1,10.0\n\
                         this_is_bad_data\n\
                         withdrawal,1,2,5.0";
    let input_file = create_temp_csv(input_content);

    let expected_output = "client,available,held,total,locked\n\
                           1,5.0000,0.0000,5.0000,false";

    let mut cmd = Command::cargo_bin("payment_engine").unwrap();
    cmd.arg(input_file.path());

    cmd.assert()
        .success()
        .stdout(predicate::str::diff(expected_output).trim())
        .stderr(predicate::str::contains("Warning: Skipping bad record:"));
}

#[rstest]
fn test_cli_write_error() {
    use std::process::Stdio;

    let input_content = "type,client,tx,amount\n\
                         deposit,1,1,10.0";
    let input_file = create_temp_csv(input_content);

    let mut child = Command::new(env!("CARGO_BIN_EXE_payment_engine"))
        .arg(input_file.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn process");

    drop(child.stdout.take());

    let output = child.wait_with_output().expect("Failed to wait for process");

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error writing accounts:"));
}
