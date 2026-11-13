// Copyright (c) 2026 Hemashushu <hippospark@gmail.com>, All rights reserved.
//
// This Source Code Form is subject to the terms of
// the Mozilla Public License version 2.0 and additional exceptions.
// For more details, see the LICENSE, LICENSE.additional, and CONTRIBUTING files.

//! The `ason2j` command-line converter for ASON/ASONB/JSON documents.

use std::{
    fs::File,
    io::{self, BufWriter, Cursor, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{CommandFactory, Parser, error::ErrorKind};
use clap_complete::{Shell, generate};

use ason_convert::{
    ason_to_asonb, ason_to_json, asonb_reader_to_ason, asonb_reader_to_json, format_json,
    json_to_ason, json_to_asonb,
};

#[derive(Clone, Copy, PartialEq, Eq)]
/// A format recognized by the command-line converter.
enum Format {
    /// Human-readable ASON text.
    Ason,
    /// Binary ASONB document data.
    Asonb,
    /// JSON or JSONC text.
    Json,
}

/// The input reader and the format hints discovered while opening it.
struct InputSource {
    /// Reads the complete input, including any bytes consumed during header detection.
    reader: Box<dyn Read>,
    /// A format inferred from the input file extension, when it is recognized.
    extension_format: Option<Format>,
    /// Whether the first eight input bytes match the ASONB document signature.
    has_binary_header: bool,
}

/// An error message paired with the process exit code expected by the CLI.
struct CliError {
    /// The status code returned to the operating system.
    code: u8,
    /// The message printed to standard error; an empty message is kept silent.
    message: String,
}

impl CliError {
    /// Creates a command-line error from a code and any value that can be displayed as text.
    fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<io::Error> for CliError {
    /// Uses the general I/O failure status for file, pipe, and terminal errors.
    fn from(error: io::Error) -> Self {
        Self::new(1, error.to_string())
    }
}

type CliResult<T> = std::result::Result<T, CliError>;

#[derive(Parser)]
#[command(
    name = "ason2j",
    version,
    about = "Convert ASON/ASONB to and from JSON"
)]
/// Arguments accepted by the `ason2j` command.
struct Ason2jArgs {
    /// Optional input file; standard input is used when this is omitted.
    #[arg(value_name = "INPUT", help = "Input file (defaults to standard input)")]
    input: Option<PathBuf>,

    /// Treats input with no recognized extension as JSON or JSONC.
    #[arg(
        short = 'r',
        long = "reverse",
        help = "Treat input without a recognized extension as JSON"
    )]
    reverse: bool,

    /// Selects ASON text output; by default, JSON input is converted to ASON.
    #[arg(
        short = 't',
        long = "text",
        conflicts_with = "binary",
        help = "Select ASON output (the default for JSON input)"
    )]
    text: bool,

    /// Selects ASONB binary output.
    #[arg(
        short = 'b',
        long = "binary",
        conflicts_with = "text",
        help = "Select ASONB output"
    )]
    binary: bool,

    /// Optional output file; standard output is used when this is omitted.
    #[arg(
        short = 'o',
        value_name = "OUTPUT",
        help = "Write output to this file (defaults to standard output)"
    )]
    output: Option<PathBuf>,

    /// Prints a shell completion script instead of converting a document.
    #[arg(
        long = "complete",
        value_name = "SHELL",
        help = "Print a completion script to standard output (bash, zsh, fish, or PowerShell)"
    )]
    complete: Option<String>,
}

/// Parses arguments and runs the `ason2j` command-line application.
///
/// Conversion errors are printed to standard error and returned as process exit codes. A
/// successful completion-script request intentionally returns status code 2 for compatibility
/// with this command's existing behavior.
fn main() -> ExitCode {
    let args = match parse_args::<Ason2jArgs>() {
        Ok(args) => args,
        Err(code) => return ExitCode::from(code),
    };
    if let Some(shell) = args.complete {
        return match emit_completion::<Ason2jArgs>(&shell, "ason2j") {
            Ok(()) => ExitCode::from(2),
            Err(error) => report_error(error),
        };
    }
    match run_ason2j_command(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => report_error(error),
    }
}

/// Parses command-line arguments and prints Clap's help or error message when parsing fails.
fn parse_args<T: Parser>() -> std::result::Result<T, u8> {
    T::try_parse().map_err(|error| {
        let code = match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => 0,
            _ => 1,
        };
        let _ = error.print();
        code
    })
}

/// Writes a completion script for one of the shells supported by Clap Complete.
fn emit_completion<T: CommandFactory>(shell: &str, name: &str) -> CliResult<()> {
    let shell = match shell.to_ascii_lowercase().as_str() {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        "powershell" => Shell::PowerShell,
        _ => {
            return Err(CliError::new(
                1,
                format!("Unsupported completion shell: {shell}"),
            ));
        }
    };
    let mut command = T::command();
    generate(shell, &mut command, name, &mut io::stdout());
    Ok(())
}

/// Determines the source and destination formats, then runs the requested ASON/ASONB/JSON conversion.
///
/// A recognized input extension takes precedence over the `-r` fallback and header detection.
/// For inputs without a recognized extension, `-r` selects JSON; otherwise the ASONB signature
/// selects binary input and text is treated as ASON. The output mode, output extension, and
/// explicit flags are checked for conflicts before processing begins.
fn run_ason2j_command(args: Ason2jArgs) -> CliResult<()> {
    let input = open_input(args.input.as_deref())?;
    let extension_format = input.extension_format;
    let source = extension_format.unwrap_or({
        if args.reverse {
            Format::Json
        } else if input.has_binary_header {
            Format::Asonb
        } else {
            Format::Ason
        }
    });
    if args.reverse && source != Format::Json {
        return Err(CliError::new(1, "-r requires JSON or JSONC input."));
    }

    let output_extension = args.output.as_deref().and_then(path_format);
    if args.text && output_extension == Some(Format::Asonb) {
        return Err(CliError::new(
            4,
            "The output file name is .asonb while -t/--text was specified.",
        ));
    }
    if args.binary && output_extension == Some(Format::Ason) {
        return Err(CliError::new(
            3,
            "The output file name is .ason while -b/--binary was specified.",
        ));
    }
    if args.text && output_extension.is_some_and(|format| format != Format::Ason) {
        return Err(CliError::new(
            1,
            "-t/--text conflicts with the selected output extension.",
        ));
    }
    if args.binary && output_extension.is_some_and(|format| format != Format::Asonb) {
        return Err(CliError::new(
            1,
            "-b conflicts with the selected output extension.",
        ));
    }
    let destination = if args.binary {
        Format::Asonb
    } else if args.text {
        Format::Ason
    } else if let Some(format) = output_extension {
        format
    } else if args.reverse {
        Format::Ason
    } else {
        Format::Json
    };
    if args.binary && destination != Format::Asonb {
        return Err(CliError::new(
            1,
            "-b conflicts with the selected output format.",
        ));
    }
    if source == destination {
        return Err(CliError::new(2, "No conversion was performed."));
    }
    let (mut output, output_is_terminal) = output_writer(args.output.as_deref())?;
    if let Some(error) = binary_terminal_write_error(destination, output_is_terminal) {
        return Err(error);
    }
    match (source, destination) {
        (Format::Ason, Format::Json) => {
            let input = read_all(input.reader)?;
            let text =
                std::str::from_utf8(&input).map_err(|error| CliError::new(1, error.to_string()))?;
            output.write_all(&ason_to_json(text).map_err(conversion_error)?.into_bytes())?;
        }
        (Format::Asonb, Format::Json) => {
            asonb_reader_to_json(input.reader, &mut output).map_err(conversion_error)?;
        }
        (Format::Json, Format::Ason) => {
            let input = read_all(input.reader)?;
            let text =
                std::str::from_utf8(&input).map_err(|error| CliError::new(1, error.to_string()))?;
            output.write_all(json_to_ason(text).map_err(conversion_error)?.as_bytes())?;
        }
        (Format::Json, Format::Asonb) => {
            let input = read_all(input.reader)?;
            let text =
                std::str::from_utf8(&input).map_err(|error| CliError::new(1, error.to_string()))?;
            output.write_all(&json_to_asonb(text).map_err(conversion_error)?)?;
        }
        (Format::Json, Format::Json) => {
            let input = read_all(input.reader)?;
            let text =
                std::str::from_utf8(&input).map_err(|error| CliError::new(1, error.to_string()))?;
            output.write_all(format_json(text).map_err(conversion_error)?.as_bytes())?;
        }
        (Format::Asonb, Format::Ason) => {
            asonb_reader_to_ason(input.reader, &mut output).map_err(conversion_error)?;
        }
        (Format::Ason, Format::Asonb) => {
            let input = read_all(input.reader)?;
            let text =
                std::str::from_utf8(&input).map_err(|error| CliError::new(1, error.to_string()))?;
            output.write_all(&ason_to_asonb(text).map_err(conversion_error)?)?;
        }
        (Format::Asonb, Format::Asonb) | (Format::Ason, Format::Ason) => {
            return Err(CliError::new(2, "No conversion was performed."));
        }
    }
    output.flush()?;
    Ok(())
}

/// Returns an error when binary ASONB data would be written directly to a terminal.
fn binary_terminal_write_error(destination: Format, output_is_terminal: bool) -> Option<CliError> {
    (destination == Format::Asonb && output_is_terminal)
        .then(|| CliError::new(5, "Writing ASONB to a terminal is not allowed."))
}

/// Opens a file or standard input and detects its extension and ASONB signature.
///
/// At most the first eight bytes are read for signature detection. Those bytes are chained back
/// in front of the remaining reader so downstream parsers receive the original complete input.
fn open_input(path: Option<&Path>) -> CliResult<InputSource> {
    let extension_format = path.and_then(path_format);
    let mut source: Box<dyn Read> = match path {
        Some(path) => Box::new(File::open(path)?),
        None => Box::new(io::stdin()),
    };
    let mut prefix = Vec::with_capacity(8);
    while prefix.len() < 8 {
        let mut buffer = [0; 8];
        let remaining = 8 - prefix.len();
        let count = source.read(&mut buffer[..remaining])?;
        if count == 0 {
            break;
        }
        prefix.extend_from_slice(&buffer[..count]);
    }
    // Keep the probed prefix in the reader; detection must not consume input from the converter.
    let has_binary_header = prefix.as_slice() == b"\0ASONB\0\x01";
    let reader = Cursor::new(prefix).chain(source);
    Ok(InputSource {
        reader: Box::new(reader),
        extension_format,
        has_binary_header,
    })
}

/// Infers an ASON, ASONB, JSON, or JSONC format from a path extension.
fn path_format(path: &Path) -> Option<Format> {
    match path.extension()?.to_str()? {
        "ason" => Some(Format::Ason),
        "asonb" => Some(Format::Asonb),
        "json" | "jsonc" => Some(Format::Json),
        _ => None,
    }
}

/// Opens an output file or wraps standard output in a buffered writer.
///
/// The returned boolean records whether the destination is a terminal, allowing the caller to
/// prevent binary output from being printed as terminal text.
fn output_writer(path: Option<&Path>) -> CliResult<(Box<dyn Write>, bool)> {
    match path {
        Some(path) => {
            let file = File::create(path)?;
            let is_terminal = file.is_terminal();
            Ok((Box::new(BufWriter::new(file)), is_terminal))
        }
        None => {
            let stdout = io::stdout();
            let is_terminal = stdout.is_terminal();
            Ok((Box::new(BufWriter::new(stdout)), is_terminal))
        }
    }
}

/// Reads all remaining bytes from an input that must be buffered for text conversion.
fn read_all(mut input: impl Read) -> CliResult<Vec<u8>> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Converts a library error to the general conversion-failure exit status.
fn conversion_error(error: impl std::fmt::Display) -> CliError {
    CliError::new(1, error.to_string())
}

/// Prints a non-empty error message and returns its configured process exit code.
fn report_error(error: CliError) -> ExitCode {
    if !error.message.is_empty() {
        eprintln!("{}", error.message);
    }
    ExitCode::from(error.code)
}

#[cfg(test)]
mod tests {
    use super::{Ason2jArgs, Format, binary_terminal_write_error};
    use clap::Parser;

    #[test]
    /// Checks `ason2j` mode parsing, including the mutually exclusive output flags.
    fn ason2j_reverse_text_and_binary_flags_are_parsed_and_exclusive() {
        assert!(
            Ason2jArgs::try_parse_from(["ason2j", "--reverse"])
                .unwrap()
                .reverse
        );
        assert!(Ason2jArgs::try_parse_from(["ason2j", "-t"]).unwrap().text);
        assert!(
            Ason2jArgs::try_parse_from(["ason2j", "--binary"])
                .unwrap()
                .binary
        );
        assert!(Ason2jArgs::try_parse_from(["ason2j", "-t", "-b"]).is_err());
    }

    #[test]
    /// Ensures ASONB bytes are not sent to an interactive terminal.
    fn terminal_binary_output_uses_exit_code_five() {
        let error = binary_terminal_write_error(Format::Asonb, true).unwrap();
        assert_eq!(error.code, 5);
        assert!(binary_terminal_write_error(Format::Ason, true).is_none());
        assert!(binary_terminal_write_error(Format::Asonb, false).is_none());
    }
}
