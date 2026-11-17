// Copyright (c) 2026 Hemashushu <hippospark@gmail.com>, All rights reserved.
//
// This Source Code Form is subject to the terms of
// the Mozilla Public License version 2.0 and additional exceptions.
// For more details, see the LICENSE, LICENSE.additional, and CONTRIBUTING files.

use std::collections::HashSet;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::error::{ConversionError, Result};

#[derive(Debug, Clone, PartialEq)]
/// A JSON value retained in source order for type inference during ASON conversion.
///
/// Unlike `serde_json::Value`, this representation keeps objects as ordered pairs and keeps
/// signed, unsigned, and floating-point numbers distinct.
pub(crate) enum JsonInput {
    /// The JSON `null` value.
    Null,
    /// A JSON boolean.
    Bool(bool),
    /// A signed integer that fits in `i64`.
    Signed(i64),
    /// An unsigned integer that fits in `u64`.
    Unsigned(u64),
    /// A JSON number with a fractional or exponent part.
    Float(f64),
    /// A JSON string.
    String(String),
    /// An array, with elements kept in their original order.
    Array(Vec<JsonInput>),
    /// An object, with fields kept in their original order.
    Object(Vec<(String, JsonInput)>),
}

/// Parses JSON with comments and trailing commas into the conversion-friendly value tree.
///
/// Duplicate object keys are rejected instead of being silently overwritten. Comment markers
/// inside quoted strings are left untouched.
///
/// # Errors
/// Returns an error for malformed JSONC, an unterminated block comment, duplicate object keys,
/// or a JSON string containing invalid Unicode escape sequences.
pub(crate) fn parse_jsonc(input: &str) -> Result<JsonInput> {
    let without_comments = strip_comments(input)?;
    let without_trailing_commas = strip_trailing_commas(&without_comments);
    serde_json::from_str(&without_trailing_commas).map_err(ConversionError::invalid_document)
}

/// Replaces line and block comments with whitespace while preserving string contents.
///
/// Whitespace replacement keeps newlines and byte positions stable, which helps the JSON parser
/// report useful locations after comments are removed. A byte-wise scan is safe here because
/// comment delimiters and quotes are ASCII bytes; all other UTF-8 bytes are copied unchanged.
fn strip_comments(input: &str) -> Result<String> {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;

    while index < bytes.len() {
        let current = bytes[index];
        if in_string {
            output.push(current);
            if escaped {
                escaped = false;
            } else if current == b'\\' {
                escaped = true;
            } else if current == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        if current == b'"' {
            in_string = true;
            output.push(current);
            index += 1;
        } else if current == b'/' && bytes.get(index + 1) == Some(&b'/') {
            output.extend_from_slice(b"  ");
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                output.push(if bytes[index] == b'\r' { b'\r' } else { b' ' });
                index += 1;
            }
        } else if current == b'/' && bytes.get(index + 1) == Some(&b'*') {
            output.extend_from_slice(b"  ");
            index += 2;
            let mut closed = false;
            while index < bytes.len() {
                if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    output.extend_from_slice(b"  ");
                    index += 2;
                    closed = true;
                    break;
                }
                output.push(if matches!(bytes[index], b'\n' | b'\r') {
                    bytes[index]
                } else {
                    b' '
                });
                index += 1;
            }
            if !closed {
                return Err(ConversionError::invalid_document(
                    "Unterminated JSONC block comment.",
                ));
            }
        } else {
            output.push(current);
            index += 1;
        }
    }

    String::from_utf8(output).map_err(ConversionError::invalid_document)
}

/// Replaces commas immediately before `]` or `}` with spaces, except inside strings.
///
/// Keeping the input length unchanged allows this pass to edit a byte buffer in place without
/// shifting any following JSON text.
fn strip_trailing_commas(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = bytes.to_vec();
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;

    while index < bytes.len() {
        let current = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if current == b'\\' {
                escaped = true;
            } else if current == b'"' {
                in_string = false;
            }
        } else if current == b'"' {
            in_string = true;
        } else if current == b',' {
            let mut next = index + 1;
            while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
                next += 1;
            }
            if matches!(bytes.get(next), Some(b']' | b'}')) {
                output[index] = b' ';
            }
        }
        index += 1;
    }

    String::from_utf8(output).expect("input is valid UTF-8")
}

impl<'de> Deserialize<'de> for JsonInput {
    /// Routes each JSON token to the corresponding `JsonInputVisitor` method.
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonInputVisitor)
    }
}

/// Builds the ordered `JsonInput` tree from the data types reported by Serde.
struct JsonInputVisitor;

impl<'de> Visitor<'de> for JsonInputVisitor {
    type Value = JsonInput;

    /// Describes the single kind of value this visitor accepts: any valid JSON value.
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    /// Treats a unit value as JSON `null`.
    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::Null)
    }

    /// Treats an absent optional value as JSON `null`.
    fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::Null)
    }

    /// Preserves a boolean token without further conversion.
    fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::Bool(value))
    }

    /// Keeps signed integers separate so later inference can choose the narrowest supported type.
    fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::Signed(value))
    }

    /// Keeps non-negative integer tokens separate from signed integers.
    fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::Unsigned(value))
    }

    /// Stores decimal and exponent-form numbers as floating-point values.
    fn visit_f64<E>(self, value: f64) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::Float(value))
    }

    /// Copies a borrowed string into the owned value tree.
    fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::String(value.to_owned()))
    }

    /// Moves an already-owned string into the value tree.
    fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::String(value))
    }

    /// Collects sequence elements in order so array type inference can inspect them later.
    fn visit_seq<A>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element()? {
            values.push(value);
        }
        Ok(JsonInput::Array(values))
    }

    /// Collects object fields in order and rejects duplicate names before they can be lost.
    fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Vec::new();
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            // JSON object keys must be unique for deterministic schema inference.
            if !keys.insert(key.clone()) {
                return Err(de::Error::custom(format!(
                    "Duplicate JSON object key {key:?}."
                )));
            }
            values.push((key, map.next_value()?));
        }
        Ok(JsonInput::Object(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Ensures comments and trailing commas are removed without changing string contents.
    fn parses_comments_and_trailing_commas_without_touching_strings() {
        let value = parse_jsonc(
            r#"{
                // line comment
                "url": "https://example.test/a/*b*/",
                "items": [1, 2,], /* block comment */
            }"#,
        )
        .unwrap();
        assert_eq!(
            value,
            JsonInput::Object(vec![
                (
                    "url".into(),
                    JsonInput::String("https://example.test/a/*b*/".into()),
                ),
                (
                    "items".into(),
                    JsonInput::Array(vec![JsonInput::Unsigned(1), JsonInput::Unsigned(2)]),
                ),
            ])
        );
    }

    #[test]
    /// Verifies malformed or ambiguous JSONC inputs are rejected.
    fn rejects_duplicate_keys_unpaired_surrogates_and_unterminated_comments() {
        assert!(parse_jsonc(r#"{"key": 1, "key": 2}"#).is_err());
        assert!(parse_jsonc(r#"{"key": "\uD800"}"#).is_err());
        assert!(parse_jsonc("{/* no end */ 1").is_err());
    }
}
