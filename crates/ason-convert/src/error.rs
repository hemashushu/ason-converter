// Copyright (c) 2026 Hemashushu <hippospark@gmail.com>, All rights reserved.
//
// This Source Code Form is subject to the terms of
// the Mozilla Public License version 2.0 and additional exceptions.
// For more details, see the LICENSE, LICENSE.additional, and CONTRIBUTING files.

use std::{
    fmt,
    io::{Read, Write},
};

/// The category of a [`ConversionError`], returned by [`ConversionError::kind`].
///
/// Use the category to decide how to handle a failure, for example, when choosing a process exit
/// status. New categories may be added in future versions, so a `match` must include a wildcard
/// arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ConversionErrorKind {
    /// The input is not a valid document in the source format.
    ///
    /// Examples include an ASON, JSON, or JSONC syntax error; an unterminated JSONC block
    /// comment; duplicate JSON object keys; and an ASONB document with a bad header, truncated
    /// data, an invalid schema, or values that do not match its schema.
    InvalidDocument,
    /// The input is valid but cannot be represented in the target format.
    ///
    /// Examples include a JSON array whose elements have incompatible types, an ASON collection
    /// whose ASONB element type cannot be inferred, a NaN or infinite number converted to JSON,
    /// and an ASONB DateTime that is out of range for ASON.
    Unconvertible,
    /// Reading from the input stream or writing to the output stream failed.
    ///
    /// Only the reader functions, such as [`asonb_reader_to_ason`](crate::asonb_reader_to_ason),
    /// perform stream I/O.
    Io,
}

/// An error raised while parsing, converting, or writing ASON, ASONB, JSON, or JSONC.
///
/// This implements [`std::error::Error`] and displays a human-readable description of the
/// failure. Use [`kind`](Self::kind) to find out whether the input was invalid, could not be
/// represented in the target format, or a stream operation failed. Errors from the ASON, ASONB,
/// JSON, and I/O libraries are converted to their display messages; their original types,
/// structured details, and error sources are not retained. The message is not a stable format
/// for programmatic matching.
///
/// # Example
///
/// ```
/// use ason_convert::{ConversionError, ConversionErrorKind, json_to_ason};
/// use std::error::Error;
///
/// let error: ConversionError = json_to_ason(r#"[1, "two"]"#).unwrap_err();
/// assert_eq!(error.kind(), ConversionErrorKind::Unconvertible);
/// assert!(!error.to_string().is_empty());
/// assert!(error.source().is_none());
///
/// let error = json_to_ason(r#"{"id": }"#).unwrap_err();
/// assert_eq!(error.kind(), ConversionErrorKind::InvalidDocument);
/// ```
#[derive(Debug)]
pub struct ConversionError {
    /// The category used by callers to distinguish failures.
    kind: ConversionErrorKind,
    /// The underlying parser, schema, serialization, or I/O error message.
    message: String,
}

impl ConversionError {
    /// Creates an error in the given category from a displayable message.
    pub(crate) fn new(kind: ConversionErrorKind, message: impl fmt::Display) -> Self {
        Self {
            kind,
            message: message.to_string(),
        }
    }

    /// Creates an error for input that is not a valid source document.
    pub(crate) fn invalid_document(message: impl fmt::Display) -> Self {
        Self::new(ConversionErrorKind::InvalidDocument, message)
    }

    /// Creates an error for valid input that the target format cannot represent.
    pub(crate) fn unconvertible(message: impl fmt::Display) -> Self {
        Self::new(ConversionErrorKind::Unconvertible, message)
    }

    /// Returns the category of this error.
    pub fn kind(&self) -> ConversionErrorKind {
        self.kind
    }
}

impl fmt::Display for ConversionError {
    /// Writes the underlying conversion error message without additional decoration.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ConversionError {}

impl From<ason::AsonError> for ConversionError {
    /// Preserves the ASON error's message and classifies it as an invalid document.
    ///
    /// ASON serialization errors are reclassified where they are produced.
    fn from(error: ason::AsonError) -> Self {
        Self::invalid_document(error)
    }
}

impl From<asonb::AsonbError> for ConversionError {
    /// Preserves the ASONB error's message and classifies it as an invalid document.
    ///
    /// ASONB serialization errors are reclassified where they are produced.
    fn from(error: asonb::AsonbError) -> Self {
        Self::invalid_document(error)
    }
}

impl From<serde_json::Error> for ConversionError {
    /// Classifies a JSON error by its category: syntax and premature end of input mean an
    /// invalid document, data errors mean an unconvertible value, and I/O errors stay I/O.
    fn from(error: serde_json::Error) -> Self {
        let kind = match error.classify() {
            serde_json::error::Category::Io => ConversionErrorKind::Io,
            serde_json::error::Category::Syntax | serde_json::error::Category::Eof => {
                ConversionErrorKind::InvalidDocument
            }
            serde_json::error::Category::Data => ConversionErrorKind::Unconvertible,
        };
        Self::new(kind, error)
    }
}

impl From<std::io::Error> for ConversionError {
    /// Converts a stream read or write failure into an I/O error.
    fn from(error: std::io::Error) -> Self {
        Self::new(ConversionErrorKind::Io, error)
    }
}

/// Wraps a reader or writer and records whether any of its operations failed.
///
/// ASON and ASONB codecs report stream failures as their own error types. The flag lets the
/// streaming functions classify such errors as [`ConversionErrorKind::Io`].
pub(crate) struct IoTracker<T> {
    /// The wrapped reader or writer.
    inner: T,
    /// Whether a read, write, or flush returned an error.
    failed: bool,
}

impl<T> IoTracker<T> {
    /// Wraps a stream with a cleared failure flag.
    pub(crate) fn new(inner: T) -> Self {
        Self {
            inner,
            failed: false,
        }
    }

    /// Records a failure and passes the result through unchanged.
    fn track<U>(&mut self, result: std::io::Result<U>) -> std::io::Result<U> {
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}

impl<R: Read> Read for IoTracker<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let result = self.inner.read(buffer);
        self.track(result)
    }
}

impl<W: Write> Write for IoTracker<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let result = self.inner.write(buffer);
        self.track(result)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let result = self.inner.flush();
        self.track(result)
    }
}

/// Runs a streaming conversion and reclassifies its error as I/O if either stream failed.
pub(crate) fn with_tracked_io<R: Read, W: Write>(
    input: R,
    output: &mut W,
    convert: impl FnOnce(&mut IoTracker<R>, &mut IoTracker<&mut W>) -> Result<()>,
) -> Result<()> {
    let mut input = IoTracker::new(input);
    let mut output = IoTracker::new(output);
    convert(&mut input, &mut output).map_err(|mut error| {
        if input.failed || output.failed {
            error.kind = ConversionErrorKind::Io;
        }
        error
    })
}

pub(crate) type Result<T> = std::result::Result<T, ConversionError>;
