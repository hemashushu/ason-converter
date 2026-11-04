// Copyright (c) 2026 Hemashushu <hippospark@gmail.com>, All rights reserved.
//
// This Source Code Form is subject to the terms of
// the Mozilla Public License version 2.0 and additional exceptions.
// For more details, see the LICENSE, LICENSE.additional, and CONTRIBUTING files.

use std::collections::HashSet;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::{ConversionError, Result};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum JsonInput {
    Null,
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    String(String),
    Array(Vec<JsonInput>),
    Object(Vec<(String, JsonInput)>),
}

pub(crate) fn parse_jsonc(input: &str) -> Result<JsonInput> {
    let without_comments = strip_comments(input)?;
    let without_trailing_commas = strip_trailing_commas(&without_comments);
    serde_json::from_str(&without_trailing_commas).map_err(Into::into)
}

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
                return Err(ConversionError("Unterminated JSONC block comment.".into()));
            }
        } else {
            output.push(current);
            index += 1;
        }
    }

    String::from_utf8(output).map_err(|error| ConversionError(error.to_string()))
}

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
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonInputVisitor)
    }
}

struct JsonInputVisitor;

impl<'de> Visitor<'de> for JsonInputVisitor {
    type Value = JsonInput;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::Null)
    }

    fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::Null)
    }

    fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::Signed(value))
    }

    fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::Unsigned(value))
    }

    fn visit_f64<E>(self, value: f64) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::Float(value))
    }

    fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E> {
        Ok(JsonInput::String(value))
    }

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

    fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = Vec::new();
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
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
    fn rejects_duplicate_keys_unpaired_surrogates_and_unterminated_comments() {
        assert!(parse_jsonc(r#"{"key": 1, "key": 2}"#).is_err());
        assert!(parse_jsonc(r#"{"key": "\uD800"}"#).is_err());
        assert!(parse_jsonc("{/* no end */ 1").is_err());
    }
}
