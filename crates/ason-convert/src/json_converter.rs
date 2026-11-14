// Copyright (c) 2026 Hemashushu <hippospark@gmail.com>, All rights reserved.
//
// This Source Code Form is subject to the terms of
// the Mozilla Public License version 2.0 and additional exceptions.
// For more details, see the LICENSE, LICENSE.additional, and CONTRIBUTING files.

use std::{
    borrow::Cow,
    io::{Cursor, Read, Write},
};

use ason::{Value as AsonValue, VariantValue};
use asonb::{
    DocumentSchema, Encoding, Field, Type, Value as AsonbValue, Variant,
    VariantValue as AsonbVariantValue,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::SecondsFormat;
use serde_json::{Map, Number, Value as JsonValue};

use crate::{
    ConversionError, Result,
    ason_converter::{from_asonb_value, with_tracked_io},
    jsonc::JsonInput,
};

#[derive(Clone)]
/// A converted value paired with the ASONB type inferred for that value.
struct Node {
    /// The value in the representation used by the ASON parser and serializer.
    value: AsonValue,
    /// The schema type needed to encode this value in ASONB.
    ty: Type,
}

/// Converts a JSON or JSONC document into ASON text using inferred types.
///
/// `input` may contain `//` and `/* ... */` comments and trailing commas, using the same
/// preprocessing as [`format_json`]. The entire document is parsed in memory, and comments and
/// original formatting are not preserved. Object keys must be unique, even in nested objects.
///
/// Integer tokens within 64-bit ranges, except `-0`, become `i32`, `i64`, or `u64` according
/// to their range. Decimal and exponent forms, `-0`, and integers outside those integer ranges
/// become `f64` when representable. Booleans and strings retain their values, and `null`
/// becomes `None`.
///
/// Arrays become Lists whose elements must have one compatible type. Numeric values are
/// promoted to a common type; mixing an integer with a floating-point value promotes the
/// integer to `f64`, which can lose precision. A negative integer and a value requiring `u64`
/// cannot share an integer type. If an array contains `null`, its non-null elements are wrapped
/// in `Some`; an otherwise uninferable List element or `Option` inner type defaults to `i32`.
///
/// A JSON object with valid ASON identifier keys becomes an Object; otherwise it becomes a
/// string-keyed Map whose values must have a common type. Valid identifiers start with an ASCII
/// letter, `_`, or a Unicode scalar at or above U+00A0; subsequent characters may also be ASCII
/// digits. The names `true`, `false`, `NaN`, `NaN_f32`, `NaN_f64`, `Inf`, `Inf_f32`, and
/// `Inf_f64` are reserved. When Map values mix null and non-null values, the latter become
/// `Some`. An Object in a collection containing Maps may itself be promoted to a Map if its
/// field values can share a type with the Map values.
///
/// Arrays of objects with valid keys share a schema: fields appear in first-seen order, and each
/// field is `Some(value)` when present and non-null or `None` when absent or null. A null row
/// makes the row itself optional. An empty object with no fields inferable from its context,
/// such as `{}` or `[{}]`, is rejected. Conversion is not generally the inverse of
/// [`ason_to_json`].
///
/// # Errors
///
/// Returns an error if parsing fails, including for duplicate keys, invalid Unicode escapes, or
/// non-representable numbers; if collection values are incompatible or an object has no
/// inferable fields; or if ASON serialization fails.
///
/// # Example
///
/// ```
/// use ason_convert::json_to_ason;
///
/// let text = json_to_ason(r#"[{"id": 1}, {"name": "Ada"}]"#)?;
/// assert!(text.contains("id: Some(1)"));
/// assert!(text.contains("id: None"));
/// assert!(text.contains("name: Some(\"Ada\")"));
/// assert_eq!(json_to_ason("-0")?, "-0.0");
/// assert!(json_to_ason("[1, 2147483648]")?.contains("1_i64"));
/// assert!(json_to_ason("[9007199254740993, 0.5]")?.contains("9007199254740992.0"));
/// assert!(json_to_ason(r#"[1, "two"]"#).is_err());
/// assert!(json_to_ason("[-1, 18446744073709551615]").is_err());
/// # Ok::<(), ason_convert::ConversionError>(())
/// ```
pub fn json_to_ason(input: &str) -> Result<String> {
    let node = convert_json_value(crate::jsonc::parse_jsonc(input)?)?;
    ason::ser_value_to_string(&node.value).map_err(ConversionError::unconvertible)
}

/// Converts a JSON or JSONC document into a complete ASONB document.
///
/// `input` is parsed in memory using the same JSONC syntax, type inference, promotion, and
/// limitations as [`json_to_ason`]. The returned bytes include the ASONB header, inferred
/// schema, and encoded root value. Collection element types, Object fields, and `Option` payload
/// types are recorded in that schema.
///
/// # Errors
///
/// Returns an error if parsing fails, including for duplicate keys, invalid Unicode escapes, or
/// non-representable numbers; if collection values are incompatible or an object has no
/// inferable fields; or if ASONB encoding fails.
///
/// # Example
///
/// ```
/// use ason_convert::{asonb_to_json, json_to_asonb};
///
/// let binary = json_to_asonb("[1, 2, 3,]")?;
/// assert_eq!(asonb_to_json(&binary)?, "[\n  1,\n  2,\n  3\n]");
/// assert!(json_to_asonb("{}").is_err());
/// # Ok::<(), ason_convert::ConversionError>(())
/// ```
pub fn json_to_asonb(input: &str) -> Result<Vec<u8>> {
    let node = convert_json_value(crate::jsonc::parse_jsonc(input)?)?;
    let schema = DocumentSchema::new(node.ty.clone());
    let value = encode_node(&node.value, &node.ty)?;
    let mut output = Vec::new();
    asonb::ser_value_to_writer(&schema, &value, &mut output)
        .map_err(ConversionError::unconvertible)?;
    Ok(output)
}

/// Parses JSON or JSONC and returns pretty-printed JSON.
///
/// Line comments extend to a line feed or the end of input; block comments cannot be nested.
/// Both are replaced with whitespace before parsing. A comma followed only by ASCII whitespace
/// and `]` or `}` is removed. Neither preprocessing pass changes quoted string contents. This
/// also accepts `[,]` and `{,}` as empty containers. Object key order is retained. Unlike
/// [`json_to_ason`], this does not infer ASON types, so empty objects and heterogeneous arrays
/// are supported.
///
/// The original whitespace, string escaping, and numeric spelling are not retained. Numbers
/// parsed as `f64`, including integers outside the supported 64-bit integer ranges, may lose
/// precision.
///
/// # Errors
///
/// Returns an error if parsing fails, including for duplicate keys, invalid Unicode escapes, or
/// numbers outside the parser's representable range, or if JSON serialization fails.
///
/// # Example
///
/// ```
/// use ason_convert::format_json;
///
/// let json = format_json("{ // comment\n \"items\": [1, true, {},], }")?;
/// assert_eq!(json, "{\n  \"items\": [\n    1,\n    true,\n    {}\n  ]\n}");
/// assert_eq!(format_json("[,]")?, "[]");
/// assert_eq!(format_json("{,}")?, "{}");
/// assert!(format_json(r#"{"key": 1, "key": 2}"#).is_err());
/// # Ok::<(), ason_convert::ConversionError>(())
/// ```
pub fn format_json(input: &str) -> Result<String> {
    let value = json_to_json_value(crate::jsonc::parse_jsonc(input)?);
    Ok(serde_json::to_string_pretty(&value)?)
}

/// Converts an ASON value to pretty-printed JSON.
///
/// The entire ASON input is parsed in memory. JSON representations are:
///
/// | ASON value | JSON representation |
/// | --- | --- |
/// | Integer, finite float, or boolean | Number or boolean |
/// | Character or string | String |
/// | DateTime | RFC 3339 string with seconds and the original offset |
/// | Bytes | Standard Base64 string with padding |
/// | List or Tuple | Array |
/// | Object | Object with the same field names |
/// | Map | Array of `{"key": ..., "value": ...}` entries in source order |
/// | `None` / `Some(value)` | `null` / the converted value |
/// | Other Enumeration, including `Result` | Object with `variant` and, for a payload, `value` |
///
/// An Enumeration's name is omitted. Tuple payloads become arrays and struct payloads become
/// objects; unit variants have no `value` field. JSON does not preserve all ASON type
/// distinctions, so [`json_to_ason`] cannot generally reverse this conversion. Large integers
/// may also lose precision in JSON consumers that use double-precision numbers.
///
/// # Errors
///
/// Returns an error for invalid ASON text, NaN or infinity, or a JSON serialization failure.
///
/// # Example
///
/// ```
/// use ason_convert::ason_to_json;
///
/// let json = ason_to_json(r#"Action::Take("item", 42)"#)?;
/// assert_eq!(
///     json,
///     "{\n  \"variant\": \"Take\",\n  \"value\": [\n    \"item\",\n    42\n  ]\n}"
/// );
/// assert_eq!(ason_to_json(r#"h"00 ff""#)?, "\"AP8=\"");
/// assert!(ason_to_json("NaN").is_err());
/// # Ok::<(), ason_convert::ConversionError>(())
/// ```
pub fn ason_to_json(input: &str) -> Result<String> {
    let value = ason::de_value_from_str(input)?;
    json_to_pretty_string(&ason_value_to_json(&value)?)
}

/// Converts exactly one complete ASONB document to pretty-printed JSON.
///
/// `input` must include the header and schema and contain no trailing bytes. The entire root
/// value, including multi-chunk Lists, is decoded in memory. The schema reconstructs ASON
/// values, which are then mapped as described by [`ason_to_json`]. DateTime values from ASONB
/// are rendered in UTC. Use [`asonb_reader_to_json`] to stream supported List roots.
///
/// # Errors
///
/// Returns an error for invalid or truncated ASONB data, trailing bytes, a schema/value
/// mismatch, an out-of-range DateTime, a non-finite float, or a JSON serialization failure.
///
/// # Example
///
/// ```
/// use ason_convert::{ason_to_asonb, asonb_to_json};
///
/// let binary = ason_to_asonb("Some(42)")?;
/// assert_eq!(asonb_to_json(&binary)?, "42");
/// # Ok::<(), ason_convert::ConversionError>(())
/// ```
pub fn asonb_to_json(input: &[u8]) -> Result<String> {
    let (schema, value) = asonb::de_value_from_reader(Cursor::new(input))?;
    let value = from_asonb_value(&schema, &schema.root, &value)?;
    json_to_pretty_string(&ason_value_to_json(&value)?)
}

/// Converts an ASONB document from `input` into UTF-8 JSON on `output`.
///
/// A multi-chunk root List, or a multi-chunk List in the selected `Result::Ok` branch, is
/// converted item by item; other roots are decoded in full. A streamed `Result::Ok` uses the
/// same `{"variant": "Ok", "value": [...]}` representation as other Enumerations. See
/// [`ason_to_json`] for the value mappings.
///
/// Reader ownership, List termination, EOF checking, buffering, and partial-output behavior
/// match [`asonb_reader_to_ason`](crate::asonb_reader_to_ason). In particular, pass
/// `&mut reader` to retain access to it, and flush `output` yourself if needed. The streaming
/// and non-streaming paths may use different whitespace and trailing newlines.
///
/// # Errors
///
/// Returns an error for invalid or truncated ASONB data, an absent root value, a schema/value
/// mismatch, a value that cannot be represented in JSON, or an input/output failure.
/// Any read or write failure is reported as [`ConversionErrorKind::Io`](crate::ConversionErrorKind::Io),
/// even when the ASONB decoder reports it as a decoding error.
///
/// # Example
///
/// This example uses the `asonb` crate to create a streamed `Result::Ok`; add it as a direct
/// dependency to run the example in another crate.
///
/// ```
/// use ason_convert::{asonb_reader_to_json, format_json};
///
/// let mut binary = Vec::new();
/// {
///     let mut writer = asonb::result_stream_writer::<i32, String, _>(&mut binary).start_ok()?;
///     writer.writes(&[1, 2])?;
///     writer.end_list()?;
/// }
///
/// let mut output = Vec::new();
/// asonb_reader_to_json(binary.as_slice(), &mut output)?;
/// let json = std::str::from_utf8(&output).unwrap();
/// assert_eq!(
///     format_json(json)?,
///     "{\n  \"variant\": \"Ok\",\n  \"value\": [\n    1,\n    2\n  ]\n}"
/// );
///
/// let mut partial = Vec::new();
/// assert!(asonb_reader_to_json(&binary[..binary.len() - 4], &mut partial).is_err());
/// assert!(!partial.is_empty()); // A failed conversion can leave partial JSON.
/// # Ok::<(), ason_convert::ConversionError>(())
/// ```
pub fn asonb_reader_to_json<R: Read, W: Write>(input: R, output: &mut W) -> Result<()> {
    with_tracked_io(input, output, |input, output| {
        let mut document = asonb::value_document_reader(input)?;
        let schema = document.schema().clone();
        let root_type = document.root_type().clone();
        if let Some(item_type) = document.item_type().cloned() {
            if document.is_streaming_result() {
                output.write_all(b"{\n    \"variant\": \"Ok\",\n    \"value\": ")?;
                write_json_items(&schema, &item_type, document, "    ", output)?;
                output.write_all(b"\n}\n")?;
            } else {
                write_json_items(&schema, &item_type, document, "", output)?;
                output.write_all(b"\n")?;
            }
        } else {
            let value = document.next().transpose()?.ok_or_else(|| {
                ConversionError::invalid_document("ASONB document has no root value.")
            })?;
            let value = from_asonb_value(&schema, &root_type, &value)?;
            output.write_all(json_to_pretty_string(&ason_value_to_json(&value)?)?.as_bytes())?;
        }
        Ok(())
    })
}

/// Writes streamed List items as a JSON array, starting at the current output position.
///
/// Each item is formatted and written before the next one is read; this preserves streaming
/// without buffering the list. `indent` is the indentation of the line holding the opening `[`.
fn write_json_items<R: Read, W: Write>(
    schema: &DocumentSchema,
    item_type: &Type,
    items: asonb::ValueDocumentReader<R>,
    indent: &str,
    output: &mut W,
) -> Result<()> {
    output.write_all(b"[")?;
    let mut first = true;
    for value in items {
        let value = value?;
        let value = from_asonb_value(schema, item_type, &value)?;
        let json = json_to_pretty_string(&ason_value_to_json(&value)?)?;
        if !first {
            output.write_all(b",")?;
        }
        output.write_all(b"\n")?;
        for (index, line) in json.lines().enumerate() {
            if index > 0 {
                output.write_all(b"\n")?;
            }
            output.write_all(indent.as_bytes())?;
            output.write_all(b"    ")?;
            output.write_all(line.as_bytes())?;
        }
        first = false;
    }
    if !first {
        output.write_all(b"\n")?;
        output.write_all(indent.as_bytes())?;
    }
    output.write_all(b"]")?;
    Ok(())
}

/// Maps each ASON value variant to its JSON representation.
///
/// ASON maps and enums need explicit JSON structures because JSON has no native equivalents.
fn ason_value_to_json(value: &AsonValue) -> Result<JsonValue> {
    let json = match value {
        AsonValue::I8(value) => JsonValue::Number((*value).into()),
        AsonValue::I16(value) => JsonValue::Number((*value).into()),
        AsonValue::I32(value) => JsonValue::Number((*value).into()),
        AsonValue::I64(value) => JsonValue::Number((*value).into()),
        AsonValue::U8(value) => JsonValue::Number((*value).into()),
        AsonValue::U16(value) => JsonValue::Number((*value).into()),
        AsonValue::U32(value) => JsonValue::Number((*value).into()),
        AsonValue::U64(value) => JsonValue::Number((*value).into()),
        AsonValue::F32(value) => number_from_f64(*value as f64)?,
        AsonValue::F64(value) => number_from_f64(*value)?,
        AsonValue::Bool(value) => JsonValue::Bool(*value),
        AsonValue::Char(value) => JsonValue::String(value.to_string()),
        AsonValue::String(value) => JsonValue::String(value.clone()),
        AsonValue::DateTime(value) => {
            JsonValue::String(value.to_rfc3339_opts(SecondsFormat::Secs, true))
        }
        AsonValue::Bytes(value) => JsonValue::String(STANDARD.encode(value)),
        AsonValue::List(values) | AsonValue::Tuple(values) => JsonValue::Array(
            values
                .iter()
                .map(ason_value_to_json)
                .collect::<Result<_>>()?,
        ),
        AsonValue::Object(fields) => {
            let mut object = Map::new();
            for (name, value) in fields {
                object.insert(name.clone(), ason_value_to_json(value)?);
            }
            JsonValue::Object(object)
        }
        AsonValue::Map(entries) => JsonValue::Array(
            entries
                .iter()
                .map(|(key, value)| {
                    let mut pair = Map::new();
                    pair.insert("key".into(), ason_value_to_json(key)?);
                    pair.insert("value".into(), ason_value_to_json(value)?);
                    Ok(JsonValue::Object(pair))
                })
                .collect::<Result<_>>()?,
        ),
        AsonValue::Enumeration { name, payload, .. } if name == "Option" => match payload {
            VariantValue::Newtype(value) => ason_value_to_json(value)?,
            _ => JsonValue::Null,
        },
        AsonValue::Enumeration {
            variant, payload, ..
        } => {
            let mut object = Map::new();
            object.insert("variant".into(), JsonValue::String(variant.clone()));
            match payload {
                VariantValue::Unit => {}
                VariantValue::Newtype(value) => {
                    object.insert("value".into(), ason_value_to_json(value)?);
                }
                VariantValue::Tuple(values) => {
                    object.insert(
                        "value".into(),
                        JsonValue::Array(
                            values
                                .iter()
                                .map(ason_value_to_json)
                                .collect::<Result<_>>()?,
                        ),
                    );
                }
                VariantValue::Struct(fields) => {
                    let mut value = Map::new();
                    for (name, item) in fields {
                        value.insert(name.clone(), ason_value_to_json(item)?);
                    }
                    object.insert("value".into(), JsonValue::Object(value));
                }
            }
            JsonValue::Object(object)
        }
    };
    Ok(json)
}

/// Creates a JSON number from a float, rejecting values outside JSON's number domain.
fn number_from_f64(value: f64) -> Result<JsonValue> {
    Number::from_f64(value)
        .map(JsonValue::Number)
        .ok_or_else(|| ConversionError::unconvertible("JSON cannot represent NaN or infinity."))
}

/// Serializes a JSON value using the standard pretty-printed layout.
fn json_to_pretty_string(value: &JsonValue) -> Result<String> {
    Ok(serde_json::to_string_pretty(value)?)
}

/// Converts the ordered parser representation into `serde_json`'s output representation.
///
/// This helper is used for JSON formatting, where no ASON-specific type inference is needed.
fn json_to_json_value(input: JsonInput) -> JsonValue {
    match input {
        JsonInput::Null => JsonValue::Null,
        JsonInput::Bool(value) => JsonValue::Bool(value),
        JsonInput::Signed(value) => JsonValue::Number(Number::from(value)),
        JsonInput::Unsigned(value) => JsonValue::Number(Number::from(value)),
        JsonInput::Float(value) => Number::from_f64(value)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        JsonInput::String(value) => JsonValue::String(value),
        JsonInput::Array(values) => {
            JsonValue::Array(values.into_iter().map(json_to_json_value).collect())
        }
        JsonInput::Object(fields) => {
            let mut object = Map::new();
            for (key, value) in fields {
                object.insert(key, json_to_json_value(value));
            }
            JsonValue::Object(object)
        }
    }
}

/// Infers an ASON value and schema type from one parsed JSON value.
///
/// JSON integer ranges are mapped to the smallest supported signed type, then to `u64` for
/// larger non-negative values. The paired type is kept with the value so collection inference
/// can later promote all members consistently.
fn convert_json_value(input: JsonInput) -> Result<Node> {
    match input {
        JsonInput::Null => Ok(option_none(Type::I32)),
        JsonInput::Bool(value) => Ok(Node {
            value: AsonValue::Bool(value),
            ty: Type::Bool,
        }),
        JsonInput::Signed(value) => Ok(if i32::try_from(value).is_ok() {
            Node {
                value: AsonValue::I32(value as i32),
                ty: Type::I32,
            }
        } else {
            Node {
                value: AsonValue::I64(value),
                ty: Type::I64,
            }
        }),
        JsonInput::Unsigned(value) => Ok(if value <= i32::MAX as u64 {
            Node {
                value: AsonValue::I32(value as i32),
                ty: Type::I32,
            }
        } else if value <= i64::MAX as u64 {
            Node {
                value: AsonValue::I64(value as i64),
                ty: Type::I64,
            }
        } else {
            Node {
                value: AsonValue::U64(value),
                ty: Type::U64,
            }
        }),
        JsonInput::Float(value) => Ok(Node {
            value: AsonValue::F64(value),
            ty: Type::F64,
        }),
        JsonInput::String(value) => Ok(Node {
            value: AsonValue::String(value),
            ty: Type::String(Encoding::Regular),
        }),
        JsonInput::Array(values) => convert_array(values),
        JsonInput::Object(fields) => convert_object(fields),
    }
}

/// Converts a JSON array and infers one shared element type for all of its items.
///
/// Arrays containing objects may be represented as rows of one object schema. If any element is
/// `null`, every row is wrapped in the same `Option<T>` type after the non-null elements have
/// established `T`.
fn convert_array(values: Vec<JsonInput>) -> Result<Node> {
    if values.is_empty() {
        return Ok(Node {
            value: AsonValue::List(Vec::new()),
            ty: Type::List(Encoding::Regular, Box::new(Type::I32)),
        });
    }

    if values
        .iter()
        .any(|value| matches!(value, JsonInput::Object(_)))
        && values
            .iter()
            .all(|value| matches!(value, JsonInput::Object(_) | JsonInput::Null))
        && let Some(node) = convert_object_array(&values)?
    {
        return Ok(node);
    }

    let contains_null = values.iter().any(|value| matches!(value, JsonInput::Null));
    // Infer the optional payload from real values first; null entries do not constrain its type.
    let mut nodes = values
        .into_iter()
        .map(convert_json_value)
        .collect::<Result<Vec<_>>>()?;

    if contains_null {
        let value_nodes = nodes
            .iter()
            .filter_map(|node| match as_option(&node.value) {
                Some(None) => None,
                Some(Some(value)) => Some(Node {
                    value: value.clone(),
                    ty: option_inner_type(&node.ty).unwrap_or(Type::I32),
                }),
                None => Some(node.clone()),
            })
            .collect::<Vec<_>>();
        let inner_type = if value_nodes.is_empty() {
            Type::I32
        } else {
            unify_nodes(&value_nodes)?.0
        };
        for node in &mut nodes {
            match as_option(&node.value) {
                Some(None) => *node = option_none(inner_type.clone()),
                Some(Some(value)) => {
                    let inner = coerce_value(value.clone(), &inner_type)?;
                    *node = option_some(inner, inner_type.clone());
                }
                None => {
                    let value = coerce_value(node.value.clone(), &inner_type)?;
                    *node = option_some(value, inner_type.clone());
                }
            }
        }
    }

    let (item_type, values) = unify_nodes(&nodes)?;
    Ok(Node {
        value: AsonValue::List(values),
        ty: Type::List(Encoding::Regular, Box::new(item_type)),
    })
}

/// Infers one row-object schema for an array of objects, ignoring `null` rows.
///
/// Fields are kept in the order they first appear. A field absent from a row becomes optional,
/// and a null row makes the row type itself optional. `None` means a field name cannot be emitted
/// as an ASON identifier, so the caller should use the general array conversion instead.
fn convert_object_array(values: &[JsonInput]) -> Result<Option<Node>> {
    let mut keys = Vec::new();
    let objects = values
        .iter()
        .filter_map(|value| match value {
            JsonInput::Object(fields) => Some(fields),
            JsonInput::Null => None,
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();
    for fields in &objects {
        if fields.iter().any(|(key, _)| !is_valid_identifier(key)) {
            return Ok(None);
        }
        for (key, _) in *fields {
            if !keys.contains(key) {
                keys.push(key.clone());
            }
        }
    }
    if keys.is_empty() {
        return Err(ConversionError::unconvertible(
            "An empty object in an array has no inferable fields.",
        ));
    }

    let mut field_types = Vec::with_capacity(keys.len());
    let mut rows = vec![Vec::with_capacity(keys.len()); objects.len()];
    for key in &keys {
        let field_values = objects
            .iter()
            .map(|fields| {
                fields
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value)
            })
            .collect::<Vec<_>>();
        let field_nodes = convert_optional_nodes(&field_values)?;
        let (field_type, field_values) = unify_nodes(&field_nodes)?;
        field_types.push(Field::new(key, field_type));
        for (row, value) in rows.iter_mut().zip(field_values) {
            row.push((key.clone(), value));
        }
    }

    let object_type = Type::Object(field_types);
    let (item_type, row_values) = if values.iter().any(|value| matches!(value, JsonInput::Null)) {
        // Keep null rows in place and wrap object rows with the same inferred Option schema.
        let item_type = option_type(object_type.clone());
        let mut rows = rows.into_iter();
        let row_values = values
            .iter()
            .map(|value| match value {
                JsonInput::Object(_) => some_value(AsonValue::Object(
                    rows.next().expect("one inferred row per object"),
                )),
                JsonInput::Null => none_value(),
                _ => unreachable!(),
            })
            .collect();
        (item_type, row_values)
    } else {
        (
            object_type,
            rows.into_iter().map(AsonValue::Object).collect(),
        )
    };
    Ok(Some(Node {
        value: AsonValue::List(row_values),
        ty: Type::List(Encoding::Regular, Box::new(item_type)),
    }))
}

/// Converts possibly missing object-field values into nodes with a shared optional schema.
///
/// Repeated nested objects are first grouped into one object schema. Missing fields and explicit
/// `null` values then use `None`, while present fields use `Some`; non-object values keep their
/// regular conversion path.
fn convert_optional_nodes(values: &[Option<&JsonInput>]) -> Result<Vec<Node>> {
    let object_count = values
        .iter()
        .filter(|value| matches!(value, Some(JsonInput::Object(_))))
        .count();
    let only_objects_or_null = values
        .iter()
        .all(|value| matches!(value, None | Some(JsonInput::Null | JsonInput::Object(_))));

    if object_count > 1 && only_objects_or_null {
        let objects = values
            .iter()
            .filter_map(|value| match value {
                Some(JsonInput::Object(_)) => Some((**value.as_ref().unwrap()).clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let Some(grouped) = convert_object_array(&objects)? else {
            if objects
                .iter()
                .any(|value| matches!(value, JsonInput::Object(fields) if fields.is_empty()))
            {
                return Err(ConversionError::unconvertible(
                    "An empty nested object cannot infer a homogeneous Map schema.",
                ));
            }
            return values
                .iter()
                .map(|value| match value {
                    None | Some(JsonInput::Null) => Ok(option_none(Type::I32)),
                    Some(value) => {
                        let node = convert_json_value((**value).clone())?;
                        Ok(option_some(node.value, node.ty))
                    }
                })
                .collect();
        };
        let AsonValue::List(object_values) = grouped.value else {
            unreachable!()
        };
        let Type::List(_, object_type) = grouped.ty else {
            unreachable!()
        };
        let mut object_values = object_values.into_iter();
        return values
            .iter()
            .map(|value| match value {
                Some(JsonInput::Object(_)) => Ok(option_some(
                    object_values.next().expect("one inferred row per object"),
                    (*object_type).clone(),
                )),
                _ => Ok(option_none((*object_type).clone())),
            })
            .collect();
    }

    values
        .iter()
        .map(|value| match value {
            None | Some(JsonInput::Null) => Ok(option_none(Type::I32)),
            Some(value) => {
                let node = convert_json_value((**value).clone())?;
                Ok(option_some(node.value, node.ty))
            }
        })
        .collect()
}

/// Converts a JSON object to an ASON object when possible, otherwise to a map.
///
/// ASON object field names must be valid identifiers. If even one key cannot be used as a field
/// name, all fields are represented as entries in a string-keyed map so that no key is changed.
fn convert_object(fields: Vec<(String, JsonInput)>) -> Result<Node> {
    if fields.is_empty() {
        return Err(ConversionError::unconvertible(
            "An empty object has no inferable ASON fields.",
        ));
    }
    if fields.iter().all(|(name, _)| is_valid_identifier(name)) {
        let mut values = Vec::with_capacity(fields.len());
        let mut types = Vec::with_capacity(fields.len());
        for (name, value) in fields {
            let node = convert_json_value(value)?;
            types.push(Field::new(&name, node.ty));
            values.push((name, node.value));
        }
        Ok(Node {
            value: AsonValue::Object(values),
            ty: Type::Object(types),
        })
    } else {
        let mut keys = Vec::with_capacity(fields.len());
        let mut nodes = Vec::with_capacity(fields.len());
        for (key, value) in fields {
            keys.push(AsonValue::String(key));
            nodes.push(convert_json_value(value)?);
        }
        let (value_type, values) = unify_nodes(&nodes)?;
        Ok(Node {
            value: AsonValue::Map(keys.into_iter().zip(values).collect()),
            ty: Type::Map(
                false,
                Box::new(Type::String(Encoding::Regular)),
                Box::new(value_type),
            ),
        })
    }
}

/// Builds the schema used to represent `Option<T>` as an ASONB enum.
fn option_type(item: Type) -> Type {
    Type::Enum(
        "Option".into(),
        vec![
            Variant::Unit("None".into()),
            Variant::Newtype("Some".into(), item),
        ],
    )
}

/// Creates the predefined `Option::None` Enumeration value.
fn none_value() -> AsonValue {
    AsonValue::Enumeration {
        name: "Option".into(),
        variant: "None".into(),
        payload: VariantValue::Unit,
    }
}

/// Creates the predefined `Option::Some(value)` Enumeration value.
fn some_value(value: AsonValue) -> AsonValue {
    AsonValue::Enumeration {
        name: "Option".into(),
        variant: "Some".into(),
        payload: VariantValue::Newtype(Box::new(value)),
    }
}

/// Views a value as the predefined `Option` Enumeration.
///
/// Returns `Some(None)` for `None`, `Some(Some(payload))` for `Some`, and `None` for any other
/// value.
fn as_option(value: &AsonValue) -> Option<Option<&AsonValue>> {
    match value {
        AsonValue::Enumeration {
            name,
            variant,
            payload,
        } if name == "Option" => match (variant.as_str(), payload) {
            ("None", VariantValue::Unit) => Some(None),
            ("Some", VariantValue::Newtype(value)) => Some(Some(value)),
            _ => None,
        },
        _ => None,
    }
}

/// Owned counterpart of [`as_option`]; returns the value unchanged when it is not an `Option`.
fn into_option(value: AsonValue) -> std::result::Result<Option<AsonValue>, AsonValue> {
    match value {
        AsonValue::Enumeration {
            name,
            variant,
            payload,
        } if name == "Option" => match (variant.as_str(), payload) {
            ("None", VariantValue::Unit) => Ok(None),
            ("Some", VariantValue::Newtype(value)) => Ok(Some(*value)),
            (_, payload) => Err(AsonValue::Enumeration {
                name,
                variant,
                payload,
            }),
        },
        value => Err(value),
    }
}

/// Creates the `None` value and its matching `Option<T>` schema node.
fn option_none(item: Type) -> Node {
    Node {
        value: none_value(),
        ty: option_type(item),
    }
}

/// Creates the `Some(value)` value and its matching `Option<T>` schema node.
fn option_some(value: AsonValue, item: Type) -> Node {
    Node {
        value: some_value(value),
        ty: option_type(item),
    }
}

/// Returns the payload type when `ty` is the generated `Option<T>` enum schema.
fn option_inner_type(ty: &Type) -> Option<Type> {
    let Type::Enum(name, variants) = ty else {
        return None;
    };
    if name != "Option" {
        return None;
    }
    variants.iter().find_map(|variant| match variant {
        Variant::Newtype(name, item) if name == "Some" => Some(item.clone()),
        _ => None,
    })
}

/// Finds a common type for nodes and coerces each value to match it.
fn unify_nodes(nodes: &[Node]) -> Result<(Type, Vec<AsonValue>)> {
    let ty = unify_type(nodes)?;
    let values = nodes
        .iter()
        .map(|node| coerce_value(node.value.clone(), &ty))
        .collect::<Result<_>>()?;
    Ok((ty, values))
}

/// Infers the most specific common type supported by the values in a collection.
///
/// This handles numeric promotion, nested lists, object/map combinations, and optional values.
/// Conversion fails if no common type can describe the values, although promotion to `f64` can
/// lose integer precision.
fn unify_type(nodes: &[Node]) -> Result<Type> {
    let Some(first) = nodes.first() else {
        return Ok(Type::I32);
    };
    if nodes.iter().all(|node| node.ty == first.ty) {
        return Ok(first.ty.clone());
    }

    if nodes.iter().all(|node| numeric_type(&node.ty)) {
        return unify_numeric_type(nodes);
    }

    if nodes.iter().all(|node| matches!(node.ty, Type::List(..))) {
        let mut item_nodes = Vec::new();
        for node in nodes {
            let (Type::List(_, item_type), AsonValue::List(values)) = (&node.ty, &node.value)
            else {
                unreachable!()
            };
            item_nodes.extend(values.iter().cloned().map(|value| Node {
                value,
                ty: (**item_type).clone(),
            }));
        }
        let item_type = unify_type(&item_nodes)?;
        return Ok(Type::List(Encoding::Regular, Box::new(item_type)));
    }

    let has_map = nodes.iter().any(|node| matches!(node.ty, Type::Map(..)));
    if has_map
        && nodes
            .iter()
            .all(|node| matches!(node.ty, Type::Map(..) | Type::Object(..)))
    {
        let mut key_nodes = Vec::new();
        let mut value_nodes = Vec::new();
        for node in nodes {
            match (&node.ty, &node.value) {
                (Type::Map(_, key_type, value_type), AsonValue::Map(entries)) => {
                    key_nodes.extend(entries.iter().map(|(key, _)| Node {
                        value: key.clone(),
                        ty: (**key_type).clone(),
                    }));
                    value_nodes.extend(entries.iter().map(|(_, value)| Node {
                        value: value.clone(),
                        ty: (**value_type).clone(),
                    }));
                }
                (Type::Object(fields), AsonValue::Object(values))
                    if fields.len() == values.len() =>
                {
                    for (field, (name, value)) in fields.iter().zip(values) {
                        key_nodes.push(Node {
                            value: AsonValue::String(name.clone()),
                            ty: Type::String(Encoding::Regular),
                        });
                        value_nodes.push(Node {
                            value: value.clone(),
                            ty: field.ty.clone(),
                        });
                    }
                }
                _ => {
                    return Err(ConversionError::unconvertible(
                        "Object or Map value does not match its inferred schema.",
                    ));
                }
            }
        }
        let key_type = if key_nodes.is_empty() {
            Type::String(Encoding::Regular)
        } else {
            unify_type(&key_nodes)?
        };
        let value_type = if value_nodes.is_empty() {
            Type::I32
        } else {
            unify_type(&value_nodes)?
        };
        return Ok(Type::Map(false, Box::new(key_type), Box::new(value_type)));
    }

    if nodes
        .iter()
        .any(|node| option_inner_type(&node.ty).is_some())
    {
        let inner_nodes = nodes
            .iter()
            .filter_map(|node| match as_option(&node.value) {
                Some(Some(value)) => Some(Node {
                    value: value.clone(),
                    ty: option_inner_type(&node.ty).unwrap(),
                }),
                Some(None) => None,
                None => Some(Node {
                    value: node.value.clone(),
                    ty: node.ty.clone(),
                }),
            })
            .collect::<Vec<_>>();
        return Ok(option_type(unify_type(&inner_nodes)?));
    }

    Err(ConversionError::unconvertible(
        "JSON collection values do not have a common ASON type.",
    ))
}

/// Reports whether a type participates in the numeric promotion rules.
fn numeric_type(ty: &Type) -> bool {
    matches!(ty, Type::I32 | Type::I64 | Type::U64 | Type::F64)
}

/// Chooses a common numeric type for all input values.
///
/// Floating-point values take precedence. Otherwise, unsigned values use `u64` unless a negative
/// signed value is present, in which case there is no common integer type. Promotion to `f64`
/// can round large integers.
fn unify_numeric_type(nodes: &[Node]) -> Result<Type> {
    let has_float = nodes.iter().any(|node| node.ty == Type::F64);
    if has_float {
        return Ok(Type::F64);
    }
    let has_u64 = nodes.iter().any(|node| node.ty == Type::U64);
    let has_i64 = nodes.iter().any(|node| node.ty == Type::I64);
    if has_u64 {
        if nodes
            .iter()
            .any(|node| numeric_as_f64(&node.value).is_some_and(|v| v < 0.0))
        {
            return Err(ConversionError::unconvertible(
                "Cannot promote negative signed integers to u64.",
            ));
        }
        return Ok(Type::U64);
    }
    Ok(if has_i64 { Type::I64 } else { Type::I32 })
}

/// Reads a numeric ASON value as `f64` for sign checks during type inference.
fn numeric_as_f64(value: &AsonValue) -> Option<f64> {
    match value {
        AsonValue::I32(value) => Some(*value as f64),
        AsonValue::I64(value) => Some(*value as f64),
        AsonValue::U64(value) => Some(*value as f64),
        AsonValue::F64(value) => Some(*value),
        _ => None,
    }
}

/// Promotes a value to the inferred type, recursively handling containers and options.
///
/// Numeric conversions are explicit so invalid promotions, especially negative integers to
/// `u64`, return a useful error rather than wrapping. Object fields can also be promoted to map
/// entries when a common collection type requires it.
fn coerce_value(value: AsonValue, ty: &Type) -> Result<AsonValue> {
    Ok(match (value, ty) {
        (AsonValue::I32(value), Type::I64) => AsonValue::I64(value as i64),
        (AsonValue::I32(value), Type::U64) => AsonValue::U64(value as u64),
        (AsonValue::I64(value), Type::U64) => {
            AsonValue::U64(u64::try_from(value).map_err(|_| {
                ConversionError::unconvertible("Cannot promote negative signed integers to u64.")
            })?)
        }
        (AsonValue::I32(value), Type::F64) => AsonValue::F64(value as f64),
        (AsonValue::I64(value), Type::F64) => AsonValue::F64(value as f64),
        (AsonValue::U64(value), Type::F64) => AsonValue::F64(value as f64),
        (AsonValue::List(values), Type::List(_, item_type)) => AsonValue::List(
            values
                .into_iter()
                .map(|value| coerce_value(value, item_type))
                .collect::<Result<_>>()?,
        ),
        (AsonValue::Map(entries), Type::Map(_, key_type, value_type)) => AsonValue::Map(
            entries
                .into_iter()
                .map(|(key, value)| {
                    Ok((
                        coerce_value(key, key_type)?,
                        coerce_value(value, value_type)?,
                    ))
                })
                .collect::<Result<_>>()?,
        ),
        (AsonValue::Object(fields), Type::Map(_, key_type, value_type)) => AsonValue::Map(
            fields
                .into_iter()
                .map(|(name, value)| {
                    Ok((
                        coerce_value(AsonValue::String(name), key_type)?,
                        coerce_value(value, value_type)?,
                    ))
                })
                .collect::<Result<_>>()?,
        ),
        (value, Type::Enum(name, variants)) if name == "Option" => {
            let item_type = variants
                .iter()
                .find_map(|variant| match variant {
                    Variant::Newtype(name, ty) if name == "Some" => Some(ty),
                    _ => None,
                })
                .ok_or_else(|| ConversionError::unconvertible("Invalid Option schema."))?;
            match into_option(value) {
                Ok(None) => none_value(),
                Ok(Some(value)) | Err(value) => some_value(coerce_value(value, item_type)?),
            }
        }
        (value, ty) if value_matches_type(&value, ty) => value,
        _ => {
            return Err(ConversionError::unconvertible(format!(
                "Cannot promote JSON value to ASON type {ty:?}."
            )));
        }
    })
}

/// Checks whether a value has the expected top-level ASON type without converting it.
fn value_matches_type(value: &AsonValue, ty: &Type) -> bool {
    matches!(
        (value, ty),
        (AsonValue::I32(_), Type::I32)
            | (AsonValue::I64(_), Type::I64)
            | (AsonValue::U64(_), Type::U64)
            | (AsonValue::F64(_), Type::F64)
            | (AsonValue::Bool(_), Type::Bool)
            | (AsonValue::String(_), Type::String(_))
            | (AsonValue::List(_), Type::List(..))
            | (AsonValue::Object(_), Type::Object(..))
            | (AsonValue::Map(_), Type::Map(..))
            | (AsonValue::Enumeration { .. }, Type::Enum(..))
    )
}

/// Encodes an inferred ASON value using its matching ASONB schema type.
///
/// Container shapes and object field counts are checked here as a final consistency guard before
/// the lower-level ASONB serializer writes the document.
fn encode_node(value: &AsonValue, ty: &Type) -> Result<AsonbValue<'static>> {
    Ok(match (value, ty) {
        (AsonValue::I32(value), Type::I32) => AsonbValue::I32(*value),
        (AsonValue::I64(value), Type::I64) => AsonbValue::I64(*value),
        (AsonValue::U64(value), Type::U64) => AsonbValue::U64(*value),
        (AsonValue::F64(value), Type::F64) => AsonbValue::F64(*value),
        (AsonValue::Bool(value), Type::Bool) => AsonbValue::Bool(*value),
        (AsonValue::String(value), Type::String(_)) => {
            AsonbValue::String(Cow::Owned(value.clone()))
        }
        (AsonValue::List(values), Type::List(_, item_type)) => AsonbValue::List(
            values
                .iter()
                .map(|value| encode_node(value, item_type))
                .collect::<Result<_>>()?,
        ),
        (AsonValue::Object(values), Type::Object(fields)) if values.len() == fields.len() => {
            AsonbValue::Object(
                values
                    .iter()
                    .zip(fields)
                    .map(|((_, value), field)| encode_node(value, &field.ty))
                    .collect::<Result<_>>()?,
            )
        }
        (AsonValue::Map(entries), Type::Map(_, key_type, value_type)) => AsonbValue::Map(
            entries
                .iter()
                .map(|(key, value)| {
                    Ok((encode_node(key, key_type)?, encode_node(value, value_type)?))
                })
                .collect::<Result<_>>()?,
        ),
        (value, Type::Enum(name, variants)) if name == "Option" => match as_option(value) {
            Some(None) => {
                let index = variants
                    .iter()
                    .position(|variant| matches!(variant, Variant::Unit(name) if name == "None"))
                    .ok_or_else(|| ConversionError::unconvertible("Invalid Option schema."))?;
                AsonbValue::Enumeration {
                    index: index as u32,
                    payload: AsonbVariantValue::Unit,
                }
            }
            Some(Some(value)) => {
                let (index, item_type) = variants
                    .iter()
                    .enumerate()
                    .find_map(|(index, variant)| match variant {
                        Variant::Newtype(name, ty) if name == "Some" => Some((index, ty)),
                        _ => None,
                    })
                    .ok_or_else(|| ConversionError::unconvertible("Invalid Option schema."))?;
                AsonbValue::Enumeration {
                    index: index as u32,
                    payload: AsonbVariantValue::Newtype(Box::new(encode_node(value, item_type)?)),
                }
            }
            None => {
                return Err(ConversionError::unconvertible(format!(
                    "JSON value does not match inferred ASONB type {ty:?}."
                )));
            }
        },
        _ => {
            return Err(ConversionError::unconvertible(format!(
                "JSON value does not match inferred ASONB type {ty:?}."
            )));
        }
    })
}

/// Reports whether a string can be emitted as an ASON identifier.
///
/// The first character must be a letter, underscore, or supported non-ASCII identifier character;
/// later characters may also be ASCII digits. Reserved literals and special float names are not
/// accepted as identifiers; the `Option`/`Result` variant shorthands are ordinary identifiers.
fn is_valid_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let valid_start = |character: char| {
        character.is_ascii_alphabetic()
            || character == '_'
            || ('\u{a0}'..='\u{d7ff}').contains(&character)
            || ('\u{e000}'..='\u{10ffff}').contains(&character)
    };
    valid_start(first)
        && chars.all(|character| valid_start(character) || character.is_ascii_digit())
        && !matches!(
            value,
            "true"
                | "false"
                | "NaN"
                | "NaN_f32"
                | "NaN_f64"
                | "Inf"
                | "Inf_f32"
                | "Inf_f64"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Checks the special JSON encodings for maps, enums, options, bytes, dates, and floats.
    fn test_ason_to_json_mappings_and_non_finite_rejection() {
        let output = ason_to_json(r#"["first": 1 "second": 2]"#).unwrap();
        let value: JsonValue = serde_json::from_str(&output).unwrap();
        assert_eq!(value[0]["key"], "first");
        assert_eq!(value[1]["key"], "second");

        let output = ason_to_json("Action::Take(\"item\" 42)").unwrap();
        let value: JsonValue = serde_json::from_str(&output).unwrap();
        assert_eq!(value["variant"], "Take");
        assert_eq!(value["value"][0], "item");
        assert_eq!(value["value"][1], 42);
        assert_eq!(ason_to_json("Option::None").unwrap(), "null");
        let output = ason_to_json("[Ok(1), Result::Err(\"failed\"), Some(2), None]").unwrap();
        assert_eq!(
            serde_json::from_str::<JsonValue>(&output).unwrap(),
            serde_json::json!([
                {"variant": "Ok", "value": 1},
                {"variant": "Err", "value": "failed"},
                2,
                null
            ])
        );
        assert_eq!(ason_to_json("h\"00 ff\"").unwrap(), "\"AP8=\"");
        assert_eq!(
            ason_to_json("d\"2024-03-16T16:30:50+08:00\"").unwrap(),
            "\"2024-03-16T16:30:50+08:00\""
        );
        assert!(ason_to_json("NaN").is_err());
        assert!(ason_to_json("Inf").is_err());
    }

    #[test]
    /// Exercises numeric promotion and nested object, map, and optional schema inference.
    fn test_json_type_inference_and_nested_schemas() {
        let output = json_to_ason("[1, 2147483648]").unwrap();
        assert_eq!(
            ason::de_value_from_str(&output).unwrap(),
            AsonValue::List(vec![AsonValue::I64(1), AsonValue::I64(2_147_483_648)])
        );
        let output = json_to_ason("[null, 1, 2]").unwrap();
        assert_eq!(
            ason::de_value_from_str(&output).unwrap(),
            AsonValue::List(vec![
                none_value(),
                some_value(AsonValue::I32(1)),
                some_value(AsonValue::I32(2)),
            ])
        );
        assert!(json_to_ason(r#"{"key": 1, "key": 2}"#).is_err());
        assert!(json_to_ason("[1, \"two\"]").is_err());
        assert!(json_to_ason("[-1, 18446744073709551615]").is_err());

        let bytes = json_to_asonb(r#"[{"id": 1}, {"id": 2, "name": "Ada"}]"#).unwrap();
        let (schema, _) = asonb::de_value_from_slice(&bytes).unwrap();
        let Type::List(_, item) = schema.root else {
            panic!("expected root List schema");
        };
        let Type::Object(fields) = *item else {
            panic!("expected Object elements");
        };
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["id", "name"]
        );
        assert!(
            fields
                .iter()
                .all(|field| matches!(&field.ty, Type::Enum(name, _) if name == "Option"))
        );

        let bytes = json_to_asonb(r#"[{"nested": {}}, {"nested": {"x": 1}}]"#).unwrap();
        let (schema, _) = asonb::de_value_from_slice(&bytes).unwrap();
        let Type::List(_, row_type) = schema.root else {
            panic!("expected root List schema");
        };
        let Type::Object(row_fields) = row_type.as_ref() else {
            panic!("expected row Object schema");
        };
        let Type::Enum(_, variants) = &row_fields[0].ty else {
            panic!("expected nested Option schema");
        };
        let Variant::Newtype(_, nested_type) = &variants[1] else {
            panic!("expected Option::Some schema");
        };
        assert!(matches!(nested_type, Type::Object(fields) if fields[0].name == "x"));

        let bytes = json_to_asonb(r#"[{"id": 1}, null]"#).unwrap();
        let (schema, value) = asonb::de_value_from_slice(&bytes).unwrap();
        let Type::List(_, item_type) = schema.root else {
            panic!("expected root List schema");
        };
        assert!(matches!(item_type.as_ref(), Type::Enum(name, _) if name == "Option"));
        assert!(
            matches!(value, AsonbValue::List(items) if matches!(items[1], AsonbValue::Enumeration { index: 0, .. }))
        );

        let bytes = json_to_asonb(r#"[{"bad-key": 1}, {"other-key": 2147483648}]"#).unwrap();
        let (schema, _) = asonb::de_value_from_slice(&bytes).unwrap();
        let Type::List(_, map_type) = schema.root else {
            panic!("expected root List schema");
        };
        assert!(matches!(map_type.as_ref(), Type::Map(_, _, value) if **value == Type::I64));

        let bytes = json_to_asonb(r#"[{"id": 1}, {"bad-key": 2147483648}]"#).unwrap();
        let (schema, value) = asonb::de_value_from_slice(&bytes).unwrap();
        let Type::List(_, map_type) = schema.root else {
            panic!("expected root List schema");
        };
        assert!(
            matches!(map_type.as_ref(), Type::Map(_, key, value) if **key == Type::String(Encoding::Regular) && **value == Type::I64)
        );
        assert_eq!(
            value,
            AsonbValue::List(vec![
                AsonbValue::Map(vec![(
                    AsonbValue::String(Cow::Borrowed("id")),
                    AsonbValue::I64(1),
                )]),
                AsonbValue::Map(vec![(
                    AsonbValue::String(Cow::Borrowed("bad-key")),
                    AsonbValue::I64(2_147_483_648),
                )]),
            ])
        );

        assert!(json_to_asonb(r#"[{"id": "one"}, {"bad-key": 2}]"#).is_err());

        let bytes = json_to_asonb(r#"{"😀": 1}"#).unwrap();
        let (schema, _) = asonb::de_value_from_slice(&bytes).unwrap();
        assert!(matches!(schema.root, Type::Object(_)));

        let bytes = json_to_asonb(r#"{"NaN": 1}"#).unwrap();
        let (schema, _) = asonb::de_value_from_slice(&bytes).unwrap();
        assert!(matches!(schema.root, Type::Map(_, _, _)));

        // Variant shorthands are ordinary identifiers and can be ASON Object keys.
        for key in ["Some", "None", "Ok", "Err"] {
            let input = format!(r#"{{"{key}": 1, "other": 2}}"#);
            let output = json_to_ason(&input).unwrap();
            assert!(matches!(
                ason::de_value_from_str(&output).unwrap(),
                AsonValue::Object(fields) if fields[0].0 == key
            ));
            let bytes = json_to_asonb(&input).unwrap();
            let (schema, _) = asonb::de_value_from_slice(&bytes).unwrap();
            assert!(matches!(schema.root, Type::Object(_)));
        }
    }

    #[test]
    /// Confirms a streamed `Result::Ok` List uses the Enumeration JSON object.
    fn test_asonb_result_stream_to_json() {
        for values in [vec![11, 13], vec![]] {
            let mut bytes = Vec::new();
            let mut writer = asonb::result_stream_writer::<i32, String, _>(&mut bytes)
                .start_ok()
                .unwrap();
            writer.writes(&values).unwrap();
            writer.end_list().unwrap();
            let mut json = Vec::new();
            asonb_reader_to_json(bytes.as_slice(), &mut json).unwrap();
            assert_eq!(
                serde_json::from_slice::<JsonValue>(&json).unwrap(),
                serde_json::json!({"variant": "Ok", "value": values})
            );
        }

        let mut bytes = Vec::new();
        asonb::result_stream_writer::<i32, String, _>(&mut bytes)
            .write_err(&"failed".to_owned())
            .unwrap();
        let mut json = Vec::new();
        asonb_reader_to_json(bytes.as_slice(), &mut json).unwrap();
        assert_eq!(
            serde_json::from_slice::<JsonValue>(&json).unwrap(),
            serde_json::json!({"variant": "Err", "value": "failed"})
        );
    }

    #[test]
    /// Confirms streamed ASONB list items form a valid JSON array.
    fn test_asonb_multichunk_list_streams_to_json() {
        let schema = DocumentSchema::new(Type::List(Encoding::MultiChunk, Box::new(Type::I32)));
        let value = AsonbValue::List(vec![
            AsonbValue::I32(1),
            AsonbValue::I32(2),
            AsonbValue::I32(3),
        ]);
        let mut bytes = Vec::new();
        asonb::ser_value_to_writer(&schema, &value, &mut bytes).unwrap();
        let mut json = Vec::new();
        asonb_reader_to_json(bytes.as_slice(), &mut json).unwrap();
        assert_eq!(
            serde_json::from_slice::<JsonValue>(&json).unwrap(),
            serde_json::json!([1, 2, 3])
        );
    }
}
