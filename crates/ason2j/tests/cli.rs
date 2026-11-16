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
    assert!(!help_text.contains("-t, --text"));
    assert!(help_text.contains("-b, --binary"));
    assert!(help_text.contains("Input file (defaults to standard input)"));
    assert!(help_text.contains("Write output to this file (defaults to standard output)"));
    assert!(help_text.contains("Print a completion script to standard output"));
    assert!(help_text.contains("PowerShell"));
    assert_eq!(invoke(&["--version"], b"").status.code(), Some(0));
    let completion = invoke(&["--complete", "bash"], b"");
    assert_eq!(completion.status.code(), Some(0));
    let completion_text = String::from_utf8_lossy(&completion.stdout);
    assert!(completion_text.contains("--binary"));
    assert!(!completion_text.contains("--text"));

    for option in ["-t", "--text"] {
        assert_eq!(invoke(&[option], b"").status.code(), Some(1));
        assert_eq!(invoke(&["-r", option], b"").status.code(), Some(1));
    }

    assert_eq!(invoke(&["-r"], b"not json").status.code(), Some(2));
    assert_eq!(invoke(&["-r"], br#"[1, "two"]"#).status.code(), Some(3));
}

#[test]
/// Keeps the default direction fixed and rejects non-JSON output names without modifying files.
fn default_conversion_only_produces_json() {
    let directory = TempDirectory::new();
    let ason = "{name: \"Ada\"}";
    let asonb = ason_convert::ason_to_asonb(ason).unwrap();
    let expected = serde_json::json!({"name": "Ada"});

    for (name, bytes) in [
        ("input.ason", ason.as_bytes()),
        ("input.asonb", asonb.as_slice()),
    ] {
        let input = directory.0.join(name);
        fs::write(&input, bytes).unwrap();
        let output = directory.0.join("output.json");
        let result = invoke(
            &[input.to_str().unwrap(), "-o", output.to_str().unwrap()],
            b"",
        );
        assert_eq!(result.status.code(), Some(0));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&output).unwrap()).unwrap(),
            expected
        );

        let result = invoke(&[], bytes);
        assert_eq!(result.status.code(), Some(0));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap(),
            expected
        );

        for name in [
            "out.ason",
            "out.asonb",
            "out.jsonc",
            "out.txt",
            "out.JSON",
            "out",
        ] {
            let output = directory.0.join(name);
            let args = [input.to_str().unwrap(), "-o", output.to_str().unwrap()];
            for existing in [false, true] {
                if existing {
                    fs::write(&output, "keep").unwrap();
                }
                let result = invoke(&args, b"");
                assert_eq!(result.status.code(), Some(2));
                assert!(String::from_utf8_lossy(&result.stderr).contains(".json"));
                assert!(result.stdout.is_empty());
                if existing {
                    assert_eq!(fs::read(&output).unwrap(), b"keep");
                    fs::remove_file(&output).unwrap();
                } else {
                    assert!(!output.exists());
                }
            }
        }
    }
}

#[test]
/// Checks the documented statuses for format conflicts, `-r` defaults, and I/O failures.
fn output_flag_and_extension_conflicts() {
    let directory = TempDirectory::new();
    let input_json = directory.0.join("input.json");
    let input_jsonc = directory.0.join("input.jsonc");
    let output_ason = directory.0.join("output.ason");
    let output_asonb = directory.0.join("output.asonb");
    fs::write(&input_json, r#"{"name":"Ada"}"#).unwrap();
    fs::write(&input_jsonc, r#"{/* comment */"name":"Ada",}"#).unwrap();

    assert_eq!(
        invoke(
            &[
                input_json.to_str().unwrap(),
                "-b",
                "-o",
                output_asonb.to_str().unwrap(),
            ],
            b"",
        )
        .status
        .code(),
        Some(1)
    );
    assert_eq!(
        invoke(
            &[
                input_json.to_str().unwrap(),
                "-r",
                "-b",
                "-o",
                output_ason.to_str().unwrap(),
            ],
            b"",
        )
        .status
        .code(),
        Some(2)
    );
    assert_eq!(
        invoke(
            &[
                input_json.to_str().unwrap(),
                "-r",
                "-o",
                output_asonb.to_str().unwrap(),
            ],
            b"",
        )
        .status
        .code(),
        Some(2)
    );
    assert!(!output_ason.exists());
    assert!(!output_asonb.exists());
    for (flags, output, required_extension) in [
        (vec!["-r", "-b"], &output_ason, ".asonb"),
        (vec!["-r"], &output_asonb, ".ason"),
    ] {
        fs::write(output, "keep").unwrap();
        let mut args = flags;
        args.extend([input_json.to_str().unwrap(), "-o", output.to_str().unwrap()]);
        let result = invoke(&args, b"");
        assert_eq!(result.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&result.stderr).contains(required_extension));
        assert_eq!(fs::read(output).unwrap(), b"keep");
    }
    for input in [&input_json, &input_jsonc] {
        assert_eq!(
            invoke(&[input.to_str().unwrap()], b"").status.code(),
            Some(2)
        );
    }
    let missing = directory.0.join("missing.json");
    assert_eq!(
        invoke(&[missing.to_str().unwrap()], b"").status.code(),
        Some(4)
    );

    let output_json = directory.0.join("output.json");
    fs::write(&output_json, "keep").unwrap();
    assert_eq!(
        invoke(
            &[
                "-r",
                input_json.to_str().unwrap(),
                "-o",
                output_json.to_str().unwrap()
            ],
            b"",
        )
        .status
        .code(),
        Some(2)
    );
    assert_eq!(fs::read_to_string(&output_json).unwrap(), "keep");

    let reversed = invoke(&["-r", input_json.to_str().unwrap()], b"");
    assert_eq!(reversed.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&reversed.stdout).contains("name: \"Ada\""));

    let reversed_file = invoke(
        &[
            "--reverse",
            input_jsonc.to_str().unwrap(),
            "-o",
            output_ason.to_str().unwrap(),
        ],
        b"",
    );
    assert_eq!(reversed_file.status.code(), Some(0));
    assert!(
        fs::read_to_string(&output_ason)
            .unwrap()
            .contains("name: \"Ada\"")
    );

    let binary_stdout = invoke(&["--reverse", "--binary"], &fs::read(&input_jsonc).unwrap());
    assert_eq!(binary_stdout.status.code(), Some(0));
    assert!(binary_stdout.stdout.starts_with(b"\0ASONB\0\x01"));
    let round_trip = invoke(&[], &binary_stdout.stdout);
    assert_eq!(round_trip.status.code(), Some(0));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&round_trip.stdout).unwrap(),
        serde_json::json!({"name": "Ada"})
    );

    for (args, name) in [
        (vec!["-r", input_json.to_str().unwrap()], "out.txt"),
        (vec!["-r", "-b", input_json.to_str().unwrap()], "out"),
    ] {
        let path = directory.0.join(name);
        let mut args = args;
        args.extend(["-o", path.to_str().unwrap()]);
        assert_eq!(invoke(&args, b"").status.code(), Some(2));
        assert!(!path.exists());
    }

    let reversed_binary = directory.0.join("reversed.asonb");
    assert_eq!(
        invoke(
            &[
                "-r",
                "-b",
                input_json.to_str().unwrap(),
                "-o",
                reversed_binary.to_str().unwrap()
            ],
            b"",
        )
        .status
        .code(),
        Some(0)
    );
    assert_eq!(&fs::read(&reversed_binary).unwrap()[..8], b"\0ASONB\0\x01");
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
