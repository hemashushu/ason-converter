// Copyright (c) 2026 Hemashushu <hippospark@gmail.com>, All rights reserved.
//
// This Source Code Form is subject to the terms of
// the Mozilla Public License version 2.0 and additional exceptions.
// For more details, see the LICENSE, LICENSE.additional, and CONTRIBUTING files.

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
enum Format {
    Ason,
    Asonb,
    Json,
}

struct InputSource {
    reader: Box<dyn Read>,
    extension_format: Option<Format>,
    has_binary_header: bool,
}

struct CliError {
    code: u8,
    message: String,
}

impl CliError {
    fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<io::Error> for CliError {
    fn from(error: io::Error) -> Self {
        Self::new(1, error.to_string())
    }
}

type CliResult<T> = std::result::Result<T, CliError>;

#[derive(Parser)]
#[command(name = "ason2b", version, about = "Convert ASON to and from ASONB")]
struct Ason2bArgs {
    #[arg(value_name = "INPUT")]
    input: Option<PathBuf>,

    #[arg(
        short = 't',
        long = "text",
        conflicts_with = "force_binary",
        help = "Force conversion from ASONB to ASON"
    )]
    force_text: bool,

    #[arg(
        short = 'b',
        long = "binary",
        conflicts_with = "force_text",
        help = "Force conversion from ASON to ASONB"
    )]
    force_binary: bool,

    #[arg(short = 'o', value_name = "OUTPUT")]
    output: Option<PathBuf>,

    #[arg(long = "complete", value_name = "SHELL")]
    complete: Option<String>,
}

#[derive(Parser)]
#[command(
    name = "ason2j",
    version,
    about = "Convert ASON/ASONB to and from JSON"
)]
struct Ason2jArgs {
    #[arg(value_name = "INPUT")]
    input: Option<PathBuf>,

    #[arg(short = 'r', long = "reverse", help = "Convert JSON to ASON/ASONB")]
    reverse: bool,

    #[arg(
        short = 't',
        long = "text",
        conflicts_with = "binary",
        help = "Indicate conversion to ASON (default)"
    )]
    text: bool,

    #[arg(
        short = 'b',
        long = "binary",
        conflicts_with = "text",
        help = "Indicate conversion to ASONB"
    )]
    binary: bool,

    #[arg(short = 'o', value_name = "OUTPUT")]
    output: Option<PathBuf>,

    #[arg(long = "complete", value_name = "SHELL")]
    complete: Option<String>,
}

pub fn run_ason2b() -> ExitCode {
    let args = match parse_args::<Ason2bArgs>() {
        Ok(args) => args,
        Err(code) => return ExitCode::from(code),
    };
    if let Some(shell) = args.complete {
        return match emit_completion::<Ason2bArgs>(&shell, "ason2b") {
            Ok(()) => ExitCode::from(2),
            Err(error) => report_error(error),
        };
    }
    match run_ason2b_command(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => report_error(error),
    }
}

pub fn run_ason2j() -> ExitCode {
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
        return Err(CliError::new(1, "ason2b does not accept JSON input."));
    }
    if args.force_text && source == Format::Ason {
        return Err(CliError::new(
            4,
            "Input is ASON while -t/--text was specified.",
        ));
    }
    if args.force_binary && source == Format::Asonb {
        return Err(CliError::new(
            3,
            "Input is ASONB while -b/--binary was specified.",
        ));
    }

    let output_extension = args.output.as_deref().and_then(ason2b_extension_format);
    if args.force_text && output_extension == Some(Format::Asonb) {
        return Err(CliError::new(1, "-t conflicts with an .asonb output path."));
    }
    if args.force_binary && output_extension == Some(Format::Ason) {
        return Err(CliError::new(1, "-b conflicts with an .ason output path."));
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
        return Err(CliError::new(1, "ason2b output must use .ason or .asonb."));
    }
    if source == destination {
        return Err(CliError::new(2, "No conversion was performed."));
    }
    let (mut output, output_is_terminal) = output_writer(args.output.as_deref())?;
    if let Some(error) = binary_terminal_write_error(destination, output_is_terminal) {
        return Err(error);
    }
    match (source, destination) {
        (Format::Ason, Format::Asonb) => {
            let input = read_all(input.reader)?;
            let text =
                std::str::from_utf8(&input).map_err(|error| CliError::new(1, error.to_string()))?;
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

    let output_extension = args.output.as_deref().and_then(ason2j_extension_format);
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

fn binary_terminal_write_error(destination: Format, output_is_terminal: bool) -> Option<CliError> {
    (destination == Format::Asonb && output_is_terminal)
        .then(|| CliError::new(5, "Writing ASONB to a terminal is not allowed."))
}

fn open_input(path: Option<&Path>) -> CliResult<InputSource> {
    let extension_format = path.and_then(ason2j_extension_format);
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
    let has_binary_header = prefix.as_slice() == b"\0ASONB\0\x01";
    let reader = Cursor::new(prefix).chain(source);
    Ok(InputSource {
        reader: Box::new(reader),
        extension_format,
        has_binary_header,
    })
}

fn ason2b_extension_format(path: &Path) -> Option<Format> {
    match path.extension()?.to_str()? {
        "ason" => Some(Format::Ason),
        "asonb" => Some(Format::Asonb),
        _ => None,
    }
}

fn ason2j_extension_format(path: &Path) -> Option<Format> {
    match path.extension()?.to_str()? {
        "ason" => Some(Format::Ason),
        "asonb" => Some(Format::Asonb),
        "json" | "jsonc" => Some(Format::Json),
        _ => None,
    }
}

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

fn read_all(mut input: impl Read) -> CliResult<Vec<u8>> {
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn conversion_error(error: impl std::fmt::Display) -> CliError {
    CliError::new(1, error.to_string())
}

fn report_error(error: CliError) -> ExitCode {
    if !error.message.is_empty() {
        eprintln!("{}", error.message);
    }
    ExitCode::from(error.code)
}

#[cfg(test)]
mod tests {
    use super::{Ason2bArgs, Ason2jArgs, Format, binary_terminal_write_error};
    use clap::Parser;

    #[test]
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
    fn terminal_binary_output_uses_exit_code_five() {
        let error = binary_terminal_write_error(Format::Asonb, true).unwrap();
        assert_eq!(error.code, 5);
        assert!(binary_terminal_write_error(Format::Ason, true).is_none());
        assert!(binary_terminal_write_error(Format::Asonb, false).is_none());
    }
}
