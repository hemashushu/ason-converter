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
    ConversionError, ConversionErrorKind, ason_to_json, asonb_reader_to_json, json_to_ason,
    json_to_asonb,
};

/// Exit status for invalid command-line arguments, including an unsupported completion shell.
const EXIT_INVALID_ARGUMENTS: u8 = 1;
/// Exit status for a document that is invalid or does not fit the requested conversion.
const EXIT_INVALID_DOCUMENT: u8 = 2;
/// Exit status for a valid document that cannot be represented in the output format.
const EXIT_FAILED_CONVERSION: u8 = 3;
/// Exit status for a failure to read input or write output.
const EXIT_IO_ERROR: u8 = 4;

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
    /// Uses the I/O failure status for file, pipe, and terminal errors.
    fn from(error: io::Error) -> Self {
        Self::new(EXIT_IO_ERROR, error.to_string())
    }
}

type CliResult<T> = std::result::Result<T, CliError>;

#[derive(Parser)]
#[command(
    name = "ason2j",
    version,
    author = "Hemashushu <hippospark@gmail.com>",
    about = "Convert ASON/ASONB to and from JSON",
    after_help = r#"
EXAMPLES
    * Convert an ASON or ASONB file to JSON
      ason2j input.ason -o output.json
      ason2j input.asonb -o output.json

    * Convert a JSON file to ASON or ASONB
      ason2j -r input.json -o output.ason
      ason2j -r -b input.json -o output.asonb

    * Convert JSON from standard input to ASON on standard output
      echo '{"key": "value"}' | ason2j -r

    * Convert a JSON file to ASON on standard output
      ason2j -r input.json

    * Convert a JSON file to ASONB and pipe it back to JSON
      ason2j -r input.json -b | ason2j
"#
)]
/// Arguments accepted by the `ason2j` command.
struct Ason2jArgs {
    /// Optional input file; standard input is used when this is omitted.
    #[arg(value_name = "INPUT", help = "Input file (defaults to standard input)")]
    input: Option<PathBuf>,

    /// Converts JSON or JSONC input to ASON, or ASONB when `-b` is specified.
    #[arg(
        short = 'r',
        long = "reverse",
        help = "Convert JSON/JSONC input to ASON (use -b for ASONB)"
    )]
    reverse: bool,

    /// Selects ASONB binary output; only valid together with `-r`.
    #[arg(
        short = 'b',
        long = "binary",
        requires = "reverse",
        help = "Select ASONB output (requires -r)"
    )]
    binary: bool,

    /// Optional output file; standard output is used when this is omitted.
    #[arg(
        short = 'o',
        long = "output",
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
/// Errors are printed to standard error and returned as the process exit codes listed in the
/// command's documentation. Printing a completion script succeeds with status 0.
fn main() -> ExitCode {
    let args = match parse_args::<Ason2jArgs>() {
        Ok(args) => args,
        Err(code) => return ExitCode::from(code),
    };
    if let Some(shell) = args.complete {
        return match emit_completion::<Ason2jArgs>(&shell, "ason2j") {
            Ok(()) => ExitCode::SUCCESS,
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
            _ => EXIT_INVALID_ARGUMENTS,
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
                EXIT_INVALID_ARGUMENTS,
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
/// Without `-r`, the input must be ASON or ASONB and the output is always JSON, regardless of
/// the output extension. A recognized input extension selects the input format; otherwise the
/// ASONB signature selects binary input and other input is treated as ASON. With `-r`, the input
/// must be JSON or JSONC and the output is ASON, or ASONB when `-b` is specified. The output
/// extension does not select the conversion format.
/// An output file name must end with `.json`, `.ason`, or `.asonb` to match the output format.
/// All format checks happen before the output is opened.
fn run_ason2j_command(args: Ason2jArgs) -> CliResult<()> {
    let input = open_input(args.input.as_deref())?;
    let (source, destination) = if args.reverse {
        if matches!(input.extension_format, Some(Format::Ason | Format::Asonb)) {
            return Err(CliError::new(
                EXIT_INVALID_DOCUMENT,
                "-r requires JSON or JSONC input.",
            ));
        }
        let destination = if args.binary {
            Format::Asonb
        } else {
            Format::Ason
        };
        (Format::Json, destination)
    } else {
        let source = match input.extension_format {
            Some(Format::Json) => {
                return Err(CliError::new(
                    EXIT_INVALID_DOCUMENT,
                    "JSON or JSONC input requires -r.",
                ));
            }
            Some(format) => format,
            None if input.has_binary_header => Format::Asonb,
            None => Format::Ason,
        };
        (source, Format::Json)
    };
    let output_extension = args.output.as_deref().and_then(output_format);
    if args.output.is_some() && output_extension != Some(destination) {
        let (extension, name) = match destination {
            Format::Ason => ("ason", "ASON"),
            Format::Asonb => ("asonb", "ASONB"),
            Format::Json => ("json", "JSON"),
        };
        return Err(CliError::new(
            EXIT_INVALID_DOCUMENT,
            format!("The output file name must end with .{extension} for {name} output."),
        ));
    }
    let (mut output, output_is_terminal) = output_writer(args.output.as_deref())?;
    if let Some(error) = binary_terminal_write_error(destination, output_is_terminal) {
        return Err(error);
    }
    match (source, destination) {
        (Format::Ason, Format::Json) => {
            let input = read_all(input.reader)?;
            let text = std::str::from_utf8(&input)
                .map_err(|error| CliError::new(EXIT_INVALID_DOCUMENT, error.to_string()))?;
            output.write_all(&ason_to_json(text).map_err(conversion_error)?.into_bytes())?;
        }
        (Format::Asonb, Format::Json) => {
            asonb_reader_to_json(input.reader, &mut output).map_err(conversion_error)?;
        }
        (Format::Json, Format::Ason) => {
            let input = read_all(input.reader)?;
            let text = std::str::from_utf8(&input)
                .map_err(|error| CliError::new(EXIT_INVALID_DOCUMENT, error.to_string()))?;
            output.write_all(json_to_ason(text).map_err(conversion_error)?.as_bytes())?;
        }
        (Format::Json, Format::Asonb) => {
            let input = read_all(input.reader)?;
            let text = std::str::from_utf8(&input)
                .map_err(|error| CliError::new(EXIT_INVALID_DOCUMENT, error.to_string()))?;
            output.write_all(&json_to_asonb(text).map_err(conversion_error)?)?;
        }
        _ => unreachable!(),
    }
    output.flush()?;
    Ok(())
}

/// Returns an error when binary ASONB data would be written directly to a terminal.
fn binary_terminal_write_error(destination: Format, output_is_terminal: bool) -> Option<CliError> {
    (destination == Format::Asonb && output_is_terminal).then(|| {
        CliError::new(
            EXIT_INVALID_DOCUMENT,
            "Writing ASONB to a terminal is not allowed.",
        )
    })
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

/// Infers an output format from a path extension; only `.ason`, `.asonb`, and `.json` qualify.
fn output_format(path: &Path) -> Option<Format> {
    match path.extension()?.to_str()? {
        "ason" => Some(Format::Ason),
        "asonb" => Some(Format::Asonb),
        "json" => Some(Format::Json),
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

/// Maps a library error to the exit status for its category.
fn conversion_error(error: ConversionError) -> CliError {
    let code = match error.kind() {
        ConversionErrorKind::InvalidDocument => EXIT_INVALID_DOCUMENT,
        ConversionErrorKind::Io => EXIT_IO_ERROR,
        _ => EXIT_FAILED_CONVERSION,
    };
    CliError::new(code, error.to_string())
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
    use super::{Ason2jArgs, EXIT_INVALID_DOCUMENT, Format, binary_terminal_write_error};
    use clap::Parser;

    #[test]
    /// Checks that binary output requires reverse mode and the removed text flag is rejected.
    fn ason2j_binary_flag_requires_reverse() {
        assert!(
            Ason2jArgs::try_parse_from(["ason2j", "--reverse"])
                .unwrap()
                .reverse
        );
        assert!(
            Ason2jArgs::try_parse_from(["ason2j", "-r", "--binary"])
                .unwrap()
                .binary
        );
        assert!(Ason2jArgs::try_parse_from(["ason2j", "-t"]).is_err());
        assert!(Ason2jArgs::try_parse_from(["ason2j", "-r", "--text"]).is_err());
        assert!(Ason2jArgs::try_parse_from(["ason2j", "--binary"]).is_err());
    }

    #[test]
    /// Ensures ASONB bytes are not sent to an interactive terminal.
    fn terminal_binary_output_is_an_invalid_document() {
        let error = binary_terminal_write_error(Format::Asonb, true).unwrap();
        assert_eq!(error.code, EXIT_INVALID_DOCUMENT);
        assert!(binary_terminal_write_error(Format::Ason, true).is_none());
        assert!(binary_terminal_write_error(Format::Asonb, false).is_none());
    }
}
