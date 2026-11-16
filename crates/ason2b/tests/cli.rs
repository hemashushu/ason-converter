// Copyright (c) 2026 Hemashushu <hippospark@gmail.com>, All rights reserved.
//
// This Source Code Form is subject to the terms of
// the Mozilla Public License version 2.0 and additional exceptions.
// For more details, see the LICENSE, LICENSE.additional, and CONTRIBUTING files.

use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

/// Creates a unique temporary directory and removes it when dropped.
struct TempDirectory(#[doc = "The temporary directory path used by the test."] PathBuf);

impl TempDirectory {
    /// Creates an empty directory unique to this test process.
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ason2b-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDirectory {
    /// Removes the temporary directory and its contents after the test finishes.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs the compiled `ason2b` binary with the given arguments and piped standard input.
fn invoke(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ason2b"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(stdin).unwrap();
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

#[test]
/// Exercises help/version output, shell completions, and completion failures.
fn help_version_and_completion() {
    let help = invoke(&["--help"], b"");
    assert_eq!(help.status.code(), Some(0));
    let help_text = format!(
        "{}{}",
        String::from_utf8_lossy(&help.stdout),
        String::from_utf8_lossy(&help.stderr)
    );
    assert!(help_text.contains("-r, --reverse"));
    assert!(!help_text.contains("-t, --text"));
    assert!(!help_text.contains("-b, --binary"));
    assert!(help_text.contains("Input file (defaults to standard input)"));
    assert!(help_text.contains("Write output to this file (defaults to standard output)"));
    assert!(help_text.contains("Print a completion script to standard output"));
    assert!(help_text.contains("PowerShell"));
    assert_eq!(invoke(&["--version"], b"").status.code(), Some(0));
    let completion = invoke(&["--complete", "zsh"], b"");
    assert_eq!(completion.status.code(), Some(0));
    let completion_text = String::from_utf8_lossy(&completion.stdout);
    assert!(completion_text.contains("--reverse"));
    assert!(!completion_text.contains("--text"));
    assert!(!completion_text.contains("--binary"));
    assert_eq!(
        invoke(&["--complete", "unknown"], b"").status.code(),
        Some(1)
    );
    for old_option in ["-t", "--text", "-b", "--binary"] {
        assert_eq!(
            invoke(&[old_option], b"").status.code(),
            Some(1),
            "{old_option} must be rejected"
        );
    }
}

#[test]
/// Checks extension-based format selection, binary round trips, and every documented exit status.
fn extension_detection_binary_round_trip_and_exit_statuses() {
    let directory = TempDirectory::new();
    let input_ason = directory.0.join("input.ason");
    let input_json = directory.0.join("input.json");
    let output_asonb = directory.0.join("output.asonb");
    let output_ason = directory.0.join("output.ason");
    fs::write(&input_ason, "{name: \"Ada\"}").unwrap();
    fs::write(&input_json, r#"{"name":"Ada"}"#).unwrap();

    let output = invoke(
        &[
            input_ason.to_str().unwrap(),
            "-o",
            output_asonb.to_str().unwrap(),
        ],
        b"",
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(&fs::read(&output_asonb).unwrap()[..8], b"\0ASONB\0\x01");

    for args in [
        vec![
            "-r",
            output_asonb.to_str().unwrap(),
            "-o",
            output_ason.to_str().unwrap(),
        ],
        vec![
            "--reverse",
            output_asonb.to_str().unwrap(),
            "-o",
            output_ason.to_str().unwrap(),
        ],
    ] {
        assert_eq!(invoke(&args, b"").status.code(), Some(0));
        assert!(
            fs::read_to_string(&output_ason)
                .unwrap()
                .contains("name: \"Ada\"")
        );
    }

    let reversed_ason = invoke(&["--reverse", input_ason.to_str().unwrap()], b"");
    assert_eq!(reversed_ason.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&reversed_ason.stderr).contains("-r/--reverse"));

    let wrong_reverse_output = invoke(
        &[
            "-r",
            output_asonb.to_str().unwrap(),
            "-o",
            output_asonb.to_str().unwrap(),
        ],
        b"",
    );
    assert_eq!(wrong_reverse_output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&wrong_reverse_output.stderr).contains("-r/--reverse"));

    let status = |args: &[&str]| invoke(args, b"").status.code();
    assert_eq!(status(&[input_json.to_str().unwrap()]), Some(2));
    fs::write(&output_ason, "keep").unwrap();
    let wrong_default_output = invoke(
        &[
            input_ason.to_str().unwrap(),
            "-o",
            output_ason.to_str().unwrap(),
        ],
        b"",
    );
    assert_eq!(wrong_default_output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&wrong_default_output.stderr).contains("-r/--reverse"));

    let default_binary_input = invoke(&[output_asonb.to_str().unwrap()], b"");
    assert_eq!(default_binary_input.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&default_binary_input.stderr).contains("-r/--reverse"));
    assert_eq!(
        invoke(&[], &fs::read(&output_asonb).unwrap()).status.code(),
        Some(2)
    );
    assert_eq!(fs::read_to_string(&output_ason).unwrap(), "keep");

    for name in ["out.json", "out.jsonc", "out.txt", "out"] {
        let path = directory.0.join(name);
        assert_eq!(
            status(&[input_ason.to_str().unwrap(), "-o", path.to_str().unwrap()]),
            Some(2)
        );
        assert!(!path.exists());
    }

    let stdin_asonb = directory.0.join("stdin.asonb");
    let stdin_status = |stdin: &[u8]| invoke(&["-o", stdin_asonb.to_str().unwrap()], stdin);
    assert_eq!(stdin_status(b"{name: ").status.code(), Some(2));
    assert_eq!(stdin_status(b"[1, \"two\"]").status.code(), Some(3));
    let missing = directory.0.join("missing.ason");
    assert_eq!(status(&[missing.to_str().unwrap()]), Some(4));
}

#[test]
/// Converts through standard streams using the default and reverse directions.
fn standard_stream_round_trip() {
    let binary = invoke(&[], b"{name: \"Ada\"}");
    assert_eq!(binary.status.code(), Some(0));
    assert!(binary.stdout.starts_with(b"\0ASONB\0\x01"));

    let text = invoke(&["-r"], &binary.stdout);
    assert_eq!(text.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&text.stdout).contains("name: \"Ada\""));
}
