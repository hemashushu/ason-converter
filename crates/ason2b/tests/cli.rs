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
            "ason-converter-{}-{}",
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

/// Runs one compiled CLI binary with the given arguments and piped standard input.
fn invoke(binary: &str, args: &[&str], stdin: &[u8]) -> Output {
    let path = match binary {
        "ason2b" => env!("CARGO_BIN_EXE_ason2b"),
        "ason2j" => env!("CARGO_BIN_EXE_ason2j"),
        _ => unreachable!(),
    };
    let mut child = Command::new(path)
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
    let output = invoke("ason2j", &["-r"], br#"{"name":"Ada","count":2}"#);
    assert_eq!(output.status.code(), Some(0));
    let text = std::str::from_utf8(&output.stdout).unwrap();
    assert!(text.contains("name: \"Ada\""));

    assert_eq!(invoke("ason2j", &["--help"], b"").status.code(), Some(0));
    let help = invoke("ason2b", &["--help"], b"");
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
    assert_eq!(invoke("ason2b", &["--version"], b"").status.code(), Some(0));
    let json_help = invoke("ason2j", &["--help"], b"");
    assert_eq!(json_help.status.code(), Some(0));
    let json_help_text = format!(
        "{}{}",
        String::from_utf8_lossy(&json_help.stdout),
        String::from_utf8_lossy(&json_help.stderr)
    );
    assert!(json_help_text.contains("-r, --reverse"));
    assert!(json_help_text.contains("-t, --text"));
    assert!(json_help_text.contains("-b, --binary"));
    assert!(json_help_text.contains("Input file (defaults to standard input)"));
    assert!(json_help_text.contains("Write output to this file (defaults to standard output)"));
    assert!(json_help_text.contains("Print a completion script to standard output"));
    assert!(json_help_text.contains("PowerShell"));
    let completion = invoke("ason2b", &["--complete", "zsh"], b"");
    assert_eq!(completion.status.code(), Some(2));
    assert!(!completion.stdout.is_empty());

    assert_eq!(
        invoke("ason2b", &["--complete", "unknown"], b"")
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        invoke("ason2j", &["-r"], b"not json").status.code(),
        Some(1)
    );
}

#[test]
/// Checks extension-based format selection, binary round trips, and documented exit statuses.
fn extension_detection_binary_round_trip_and_exit_statuses() {
    let directory = TempDirectory::new();
    let input_ason = directory.0.join("input.ason");
    let input_json = directory.0.join("input.json");
    let output_asonb = directory.0.join("output.asonb");
    let output_ason = directory.0.join("output.ason");
    fs::write(&input_ason, "{name: \"Ada\"}").unwrap();
    fs::write(&input_json, r#"{"name":"Ada"}"#).unwrap();

    let output = invoke(
        "ason2b",
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
        "ason2b",
        &[
            "--binary",
            input_ason.to_str().unwrap(),
            "-o",
            forced_asonb.to_str().unwrap(),
        ],
        b"",
    );
    assert_eq!(output.status.code(), Some(0));

    let output = invoke(
        "ason2b",
        &[
            output_asonb.to_str().unwrap(),
            "-o",
            output_ason.to_str().unwrap(),
        ],
        b"",
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(
        fs::read_to_string(&output_ason)
            .unwrap()
            .contains("name: \"Ada\"")
    );

    let output = invoke(
        "ason2b",
        &[
            "--text",
            output_asonb.to_str().unwrap(),
            "-o",
            output_ason.to_str().unwrap(),
        ],
        b"",
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(
        fs::read_to_string(&output_ason)
            .unwrap()
            .contains("name: \"Ada\"")
    );

    assert_eq!(
        invoke("ason2b", &["-b", output_asonb.to_str().unwrap()], b"")
            .status
            .code(),
        Some(3)
    );
    assert_eq!(
        invoke("ason2b", &["--text", input_ason.to_str().unwrap()], b"")
            .status
            .code(),
        Some(4)
    );
    assert_eq!(
        invoke(
            "ason2j",
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
            "ason2j",
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
    assert_eq!(
        invoke(
            "ason2b",
            &[
                "-t",
                output_asonb.to_str().unwrap(),
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
        invoke("ason2b", &["-t", "-b", input_ason.to_str().unwrap()], b"")
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        invoke(
            "ason2b",
            &[
                input_ason.to_str().unwrap(),
                "-o",
                output_ason.to_str().unwrap(),
            ],
            b"",
        )
        .status
        .code(),
        Some(2)
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
        "ason2j",
        &[input.to_str().unwrap(), "-o", output.to_str().unwrap()],
        b"",
    );
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(output).unwrap()).unwrap(),
        serde_json::json!([3, 5, 8])
    );
}
