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

use asonb::{DocumentSchema, Encoding, Type, Value};

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

/// Creates a unique temporary directory and removes it when dropped.
struct TempDirectory(#[doc = "The temporary directory path used by the test."] PathBuf);

impl TempDirectory {
    /// Creates an empty directory unique to this test process.
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ason2j-{}-{}",
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

/// Runs the compiled `ason2j` binary with the given arguments and piped standard input.
fn invoke(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ason2j"))
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
/// Exercises stdin conversion, help/version output, shell completions, and parse failures.
fn stdin_conversion_help_version_and_completion() {
    let output = invoke(&["-r"], br#"{"name":"Ada","count":2}"#);
    assert_eq!(output.status.code(), Some(0));
    let text = std::str::from_utf8(&output.stdout).unwrap();
    assert!(text.contains("name: \"Ada\""));

    let help = invoke(&["--help"], b"");
    assert_eq!(help.status.code(), Some(0));
    let help_text = format!(
        "{}{}",
        String::from_utf8_lossy(&help.stdout),
        String::from_utf8_lossy(&help.stderr)
    );
    assert!(help_text.contains("-r, --reverse"));
    assert!(help_text.contains("-t, --text"));
    assert!(help_text.contains("-b, --binary"));
    assert!(help_text.contains("Input file (defaults to standard input)"));
    assert!(help_text.contains("Write output to this file (defaults to standard output)"));
    assert!(help_text.contains("Print a completion script to standard output"));
    assert!(help_text.contains("PowerShell"));
    assert_eq!(invoke(&["--version"], b"").status.code(), Some(0));
    let completion = invoke(&["--complete", "bash"], b"");
    assert_eq!(completion.status.code(), Some(2));
    assert!(!completion.stdout.is_empty());

    assert_eq!(invoke(&["-r"], b"not json").status.code(), Some(1));
}

#[test]
/// Checks that output flags conflicting with the output extension use the documented statuses.
fn output_flag_and_extension_conflicts() {
    let directory = TempDirectory::new();
    let input_ason = directory.0.join("input.ason");
    let input_json = directory.0.join("input.json");
    let output_ason = directory.0.join("output.ason");
    let output_asonb = directory.0.join("output.asonb");
    fs::write(&input_ason, "{name: \"Ada\"}").unwrap();
    fs::write(&input_json, r#"{"name":"Ada"}"#).unwrap();

    assert_eq!(
        invoke(
            &[
                input_ason.to_str().unwrap(),
                "-b",
                "-o",
                output_ason.to_str().unwrap(),
            ],
            b"",
        )
        .status
        .code(),
        Some(3)
    );
    assert_eq!(
        invoke(
            &[
                input_json.to_str().unwrap(),
                "-t",
                "-o",
                output_asonb.to_str().unwrap(),
            ],
            b"",
        )
        .status
        .code(),
        Some(4)
    );
}

#[test]
/// Verifies a multi-chunk ASONB list is converted to JSON by the CLI.
fn asonb_multichunk_cli_conversion_streams_to_json() {
    let directory = TempDirectory::new();
    let input = directory.0.join("stream.asonb");
    let output = directory.0.join("stream.json");
    let schema = DocumentSchema::new(Type::List(Encoding::MultiChunk, Box::new(Type::I32)));
    let value = Value::List(vec![Value::I32(3), Value::I32(5), Value::I32(8)]);
    let mut bytes = Vec::new();
    asonb::ser_value_to_writer(&schema, &value, &mut bytes).unwrap();
    fs::write(&input, bytes).unwrap();

    let result = invoke(
        &[input.to_str().unwrap(), "-o", output.to_str().unwrap()],
        b"",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(output).unwrap()).unwrap(),
        serde_json::json!([3, 5, 8])
    );
}
