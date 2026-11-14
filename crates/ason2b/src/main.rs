// Copyright (c) 2026 Hemashushu <hippospark@gmail.com>, All rights reserved.
//
// This Source Code Form is subject to the terms of
// the Mozilla Public License version 2.0 and additional exceptions.
// For more details, see the LICENSE, LICENSE.additional, and CONTRIBUTING files.

//! The `ason2b` command-line converter for ASON/ASONB documents.

use std::{
    fs::File,
    io::{self, BufWriter, Cursor, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{CommandFactory, Parser, error::ErrorKind};
use clap_complete::{Shell, generate};

use ason_convert::{ConversionError, ConversionErrorKind, ason_to_asonb, asonb_reader_to_ason};

/// Exit status for invalid command-line arguments, including an unsupported completion shell.
const EXIT_INVALID_ARGUMENTS: u8 = 1;
/// Exit status for a document that is invalid or does not fit the requested conversion.
const EXIT_INVALID_DOCUMENT: u8 = 2;
/// Exit status for a valid document that cannot be represented in the output format.
const EXIT_FAILED_CONVERSION: u8 = 3;
/// Exit status for a failure to read input or write output.
const EXIT_IO_ERROR: u8 = 4;
/// Exit status when the input already has the output format.
const EXIT_NO_CONVERSION: u8 = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
/// A format recognized by the command-line converter.
enum Format {
    /// Human-readable ASON text.
    Ason,
    /// Binary ASONB document data.
    Asonb,
    /// JSON or JSONC text, which is recognized only so it can be rejected.
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
    name = "ason2b",
    version,
    author = "Hemashushu <hippospark@gmail.com>",
    about = "Convert ASON to and from ASONB",
    after_help = r#"
EXAMPLES
    * Convert an ASON file to ASONB
      ason2b input.ason -o output.asonb

    * Convert an ASONB file to ASON
      ason2b input.asonb -o output.ason

    * Convert ASON from standard input to an ASONB file
      echo '{key: "value"}' | ason2b -o output.asonb

    * Convert ASON from standard input to ASONB, then back to ASON
      echo '{key: "value"}' | ason2b | ason2b
"#
)]
/// Arguments accepted by the `ason2b` command.
struct Ason2bArgs {
    /// Optional input file; standard input is used when this is omitted.
    #[arg(value_name = "INPUT", help = "Input file (defaults to standard input)")]
    input: Option<PathBuf>,

    /// Requires ASONB input and selects ASON text output.
    #[arg(
        short = 't',
        long = "text",
        conflicts_with = "force_binary",
        help = "Force conversion from ASONB to ASON"
    )]
    force_text: bool,

    /// Requires ASON input and selects ASONB binary output.
    #[arg(
        short = 'b',
        long = "binary",
        conflicts_with = "force_text",
        help = "Force conversion from ASON to ASONB"
    )]
    force_binary: bool,

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

/// Parses arguments and runs the `ason2b` command-line application.
///
/// Errors are printed to standard error and returned as the process exit codes listed in the
/// command's documentation. Printing a completion script succeeds with status 0.
fn main() -> ExitCode {
    let args = match parse_args::<Ason2bArgs>() {
        Ok(args) => args,
        Err(code) => return ExitCode::from(code),
    };
    if let Some(shell) = args.complete {
        return match emit_completion::<Ason2bArgs>(&shell, "ason2b") {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => report_error(error),
        };
    }
    match run_ason2b_command(args) {
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

/// Determines the source and destination formats, then runs the requested ASON/ASONB conversion.
///
/// A recognized file extension takes precedence over binary-header detection. Without an
/// extension, the ASONB header determines the source format; otherwise the input is treated as
/// ASON. Explicit mode flags and the output extension select the destination, with conflicts
/// rejected before any conversion is written.
fn run_ason2b_command(args: Ason2bArgs) -> CliResult<()> {
    let input = open_input(args.input.as_deref())?;
    let source = input
        .extension_format
        .unwrap_or(if input.has_binary_header {
            Format::Asonb
        } else {
            Format::Ason
        });
    if source == Format::Json {
        return Err(CliError::new(
            EXIT_INVALID_DOCUMENT,
            "ason2b does not accept JSON input.",
        ));
    }
    if args.force_text && source == Format::Ason {
        return Err(CliError::new(
            EXIT_INVALID_DOCUMENT,
            "Input is ASON while -t/--text was specified.",
        ));
    }
    if args.force_binary && source == Format::Asonb {
        return Err(CliError::new(
            EXIT_INVALID_DOCUMENT,
            "Input is ASONB while -b/--binary was specified.",
        ));
    }

    let output_extension = args.output.as_deref().and_then(output_extension_format);
    if args.force_text && output_extension == Some(Format::Asonb) {
        return Err(CliError::new(
            EXIT_INVALID_DOCUMENT,
            "-t conflicts with an .asonb output path.",
        ));
    }
    if args.force_binary && output_extension == Some(Format::Ason) {
        return Err(CliError::new(
            EXIT_INVALID_DOCUMENT,
            "-b conflicts with an .ason output path.",
        ));
    }
    let destination = if args.force_text {
        Format::Ason
    } else if args.force_binary {
        Format::Asonb
    } else if let Some(format) = output_extension {
        format
    } else if source == Format::Asonb {
        Format::Ason
    } else {
        Format::Asonb
    };
    if destination == Format::Json {
        return Err(CliError::new(
            EXIT_INVALID_DOCUMENT,
            "ason2b output must use .ason or .asonb.",
        ));
    }
    if source == destination {
        return Err(CliError::new(
            EXIT_NO_CONVERSION,
            "No conversion was performed.",
        ));
    }
    let (mut output, output_is_terminal) = output_writer(args.output.as_deref())?;
    if let Some(error) = binary_terminal_write_error(destination, output_is_terminal) {
        return Err(error);
    }
    match (source, destination) {
        (Format::Ason, Format::Asonb) => {
            let input = read_all(input.reader)?;
            let text = std::str::from_utf8(&input)
                .map_err(|error| CliError::new(EXIT_INVALID_DOCUMENT, error.to_string()))?;
            output.write_all(&ason_to_asonb(text).map_err(conversion_error)?)?;
        }
        (Format::Asonb, Format::Ason) => {
            asonb_reader_to_ason(input.reader, &mut output).map_err(conversion_error)?;
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
    let extension_format = path.and_then(input_extension_format);
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

/// Infers an ASON or ASONB format from an output path extension.
fn output_extension_format(path: &Path) -> Option<Format> {
    match path.extension()?.to_str()? {
        "ason" => Some(Format::Ason),
        "asonb" => Some(Format::Asonb),
        _ => None,
    }
}

/// Infers an input format from a path extension, recognizing JSON so it can be rejected.
fn input_extension_format(path: &Path) -> Option<Format> {
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
    use super::{Ason2bArgs, EXIT_INVALID_DOCUMENT, Format, binary_terminal_write_error};
    use clap::Parser;

    #[test]
    /// Checks that `ason2b` accepts either mode flag but rejects using both together.
    fn ason2b_text_and_binary_flags_are_mutually_exclusive() {
        let text_short = Ason2bArgs::try_parse_from(["ason2b", "-t"]).unwrap();
        assert!(text_short.force_text);

        let text = Ason2bArgs::try_parse_from(["ason2b", "--text"]).unwrap();
        assert!(text.force_text);
        assert!(!text.force_binary);

        let binary = Ason2bArgs::try_parse_from(["ason2b", "--binary"]).unwrap();
        assert!(!binary.force_text);
        assert!(binary.force_binary);

        let binary_short = Ason2bArgs::try_parse_from(["ason2b", "-b"]).unwrap();
        assert!(binary_short.force_binary);

        assert!(Ason2bArgs::try_parse_from(["ason2b", "-t", "-b"]).is_err());
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
