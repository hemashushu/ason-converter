// Copyright (c) 2026 Hemashushu <hippospark@gmail.com>, All rights reserved.
//
// This Source Code Form is subject to the terms of
// the Mozilla Public License version 2.0 and additional exceptions.
// For more details, see the LICENSE, LICENSE.additional, and CONTRIBUTING files.

//! Convert values between ASON, ASONB, and JSON/JSONC.
//!
//! # Choosing a conversion
//!
//! - [`ason_to_asonb`] and [`asonb_to_ason`] convert between ASON text and complete ASONB
//!   documents. [`asonb_reader_to_ason`] also supports streaming certain ASONB Lists.
//! - [`ason_to_json`], [`asonb_to_json`], and [`asonb_reader_to_json`] produce JSON.
//! - [`json_to_ason`] and [`json_to_asonb`] accept JSON or JSONC and infer ASON types.
//!   [`format_json`] accepts the same input without converting it to ASON.
//!
//! All functions return a [`ConversionError`] on failure; its [`ConversionErrorKind`] tells
//! invalid input apart from unconvertible values and I/O failures. The string and byte-vector functions
//! materialize their inputs and outputs; the reader functions describe when they can stream.
//!
//! # Conversion fidelity
//!
//! Comments and original formatting are not retained. JSON has fewer types than ASON, so
//! [`ason_to_json`] documents representations for maps, enums, options, and other ASON values;
//! converting that JSON back does not generally recover the original ASON types. JSON-to-ASON
//! conversions also infer homogeneous collection types and may promote numbers, potentially
//! losing precision. ASONB stores DateTime values as Unix timestamp seconds, so the original
//! UTC offset and leap-second representation are not retained.
//!
//! # Example
//!
//! ```
//! use ason_convert::{asonb_to_json, json_to_asonb};
//!
//! let binary = json_to_asonb(r#"{ "answer": 42, }"#)?;
//! let json = asonb_to_json(&binary)?;
//! assert_eq!(json, "{\n  \"answer\": 42\n}");
//! # Ok::<(), ason_convert::ConversionError>(())
//! ```

mod ason_converter;
mod json_converter;
mod jsonc;

pub(crate) use ason_converter::Result;
pub use ason_converter::{
    ConversionError, ConversionErrorKind, ason_to_asonb, asonb_reader_to_ason, asonb_to_ason,
};
pub use json_converter::{
    ason_to_json, asonb_reader_to_json, asonb_to_json, format_json, json_to_ason, json_to_asonb,
};
