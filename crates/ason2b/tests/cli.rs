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
    assert!(help_text.contains("-t, --text"));
    assert!(help_text.contains("-b, --binary"));
    assert!(help_text.contains("Input file (defaults to standard input)"));
    assert!(help_text.contains("Write output to this file (defaults to standard output)"));
    assert!(help_text.contains("Print a completion script to standard output"));
    assert!(help_text.contains("PowerShell"));
    assert_eq!(invoke(&["--version"], b"").status.code(), Some(0));
    let completion = invoke(&["--complete", "zsh"], b"");
    assert_eq!(completion.status.code(), Some(0));
    assert!(!completion.stdout.is_empty());
    assert_eq!(
        invoke(&["--complete", "unknown"], b"").status.code(),
        Some(1)
    );
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

    let forced_asonb = directory.0.join("forced.asonb");
    let output = invoke(
        &[
            "--binary",
            input_ason.to_str().unwrap(),
            "-o",
            forced_asonb.to_str().unwrap(),
        ],
        b"",
    );
    assert_eq!(output.status.code(), Some(0));

    for args in [
        vec![
            output_asonb.to_str().unwrap(),
            "-o",
            output_ason.to_str().unwrap(),
        ],
        vec![
            "--text",
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

    let status = |args: &[&str]| invoke(args, b"").status.code();
    assert_eq!(status(&["-b", output_asonb.to_str().unwrap()]), Some(2));
    assert_eq!(status(&["--text", input_ason.to_str().unwrap()]), Some(2));
    assert_eq!(
        status(&[
            "-t",
            output_asonb.to_str().unwrap(),
            "-o",
            output_asonb.to_str().unwrap(),
        ]),
        Some(2)
    );
    assert_eq!(status(&["-t", "-b", input_ason.to_str().unwrap()]), Some(1));
    assert_eq!(status(&[input_json.to_str().unwrap()]), Some(2));
    assert_eq!(
        status(&[
            input_ason.to_str().unwrap(),
            "-o",
            output_ason.to_str().unwrap(),
        ]),
        Some(5)
    );

    let stdin_status = |stdin: &[u8]| invoke(&["-o", forced_asonb.to_str().unwrap()], stdin);
    assert_eq!(stdin_status(b"{name: ").status.code(), Some(2));
    assert_eq!(stdin_status(b"[1, \"two\"]").status.code(), Some(3));
    let missing = directory.0.join("missing.ason");
    assert_eq!(status(&[missing.to_str().unwrap()]), Some(4));
}
