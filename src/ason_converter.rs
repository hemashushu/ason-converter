// Copyright (c) 2026 Hemashushu <hippospark@gmail.com>, All rights reserved.
//
// This Source Code Form is subject to the terms of
// the Mozilla Public License version 2.0 and additional exceptions.
// For more details, see the LICENSE, LICENSE.additional, and CONTRIBUTING files.

use std::{
    borrow::Cow,
    fmt,
    io::{Cursor, Read, Write},
};

use ason::{Value as AsonValue, VariantValue};
use asonb::{DocumentSchema, Encoding, Field, Type, Value as AsonbValue, Variant};
use chrono::{DateTime, Utc};

#[derive(Debug)]
pub struct ConversionError(pub(crate) String);

impl fmt::Display for ConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ConversionError {}

impl From<ason::AsonError> for ConversionError {
    fn from(error: ason::AsonError) -> Self {
        Self(error.to_string())
    }
}

impl From<asonb::AsonbError> for ConversionError {
    fn from(error: asonb::AsonbError) -> Self {
        Self(error.to_string())
    }
}

impl From<serde_json::Error> for ConversionError {
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<std::io::Error> for ConversionError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

pub(crate) type Result<T> = std::result::Result<T, ConversionError>;

pub fn ason_to_asonb(input: &str) -> Result<Vec<u8>> {
    let value = ason::de_value_from_str(input)?;
    let (root_type, binary_value) = to_asonb_value(&value)?;
    let schema = DocumentSchema::new(root_type);
    let mut output = Vec::new();
    asonb::ser_value_to_writer(&schema, &binary_value, &mut output)?;
    Ok(output)
}

pub fn asonb_to_ason(input: &[u8]) -> Result<String> {
    let (schema, value) = asonb::de_value_from_reader(Cursor::new(input))?;
    let value = from_asonb_value(&schema, &schema.root, &value)?;
    Ok(ason::ser_value_to_string(&value)?)
}

pub fn asonb_reader_to_ason<R: Read, W: Write>(input: R, output: &mut W) -> Result<()> {
    let mut document = asonb::value_document_reader(input)?;
    let schema = document.schema().clone();
    let root_type = document.root_type().clone();
    if let Type::List(_, item_type) = &root_type
        && document.is_streaming_list()
    {
        let mut writer = ason::list_stream_writer::<AsonValue, _>(output);
        writer.start_list()?;
        for value in document {
            let value = value?;
            let value = from_asonb_value(&schema, item_type, &value)?;
            writer.write_value(&value)?;
        }
        writer.end_list()?;
    } else {
        let value = document
            .next()
            .transpose()?
            .ok_or_else(|| ConversionError("ASONB document has no root value.".into()))?;
        let value = from_asonb_value(&schema, &root_type, &value)?;
        ason::ser_value_to_writer(&value, output)?;
    }
    Ok(())
}

fn to_asonb_value(value: &AsonValue) -> Result<(Type, AsonbValue<'static>)> {
    let converted = match value {
        AsonValue::I8(value) => (Type::I8, AsonbValue::I8(*value)),
        AsonValue::I16(value) => (Type::I16, AsonbValue::I16(*value)),
        AsonValue::I32(value) => (Type::I32, AsonbValue::I32(*value)),
        AsonValue::I64(value) => (Type::I64, AsonbValue::I64(*value)),
        AsonValue::U8(value) => (Type::U8, AsonbValue::U8(*value)),
        AsonValue::U16(value) => (Type::U16, AsonbValue::U16(*value)),
        AsonValue::U32(value) => (Type::U32, AsonbValue::U32(*value)),
        AsonValue::U64(value) => (Type::U64, AsonbValue::U64(*value)),
        AsonValue::F32(value) => (Type::F32, AsonbValue::F32(*value)),
        AsonValue::F64(value) => (Type::F64, AsonbValue::F64(*value)),
        AsonValue::Bool(value) => (Type::Bool, AsonbValue::Bool(*value)),
        AsonValue::Char(value) => (Type::Char, AsonbValue::Char(*value)),
        AsonValue::String(value) => (
            Type::String(Encoding::Regular),
            AsonbValue::String(Cow::Owned(value.clone())),
        ),
        AsonValue::DateTime(value) => (Type::DateTime, AsonbValue::DateTime(value.timestamp())),
        AsonValue::Bytes(value) => (
            Type::Bytes(Encoding::Regular),
            AsonbValue::Bytes(Cow::Owned(value.clone())),
        ),
        AsonValue::List(values) => {
            let (item_type, items) = homogeneous_values(values)?;
            (
                Type::List(Encoding::Regular, Box::new(item_type)),
                AsonbValue::List(items),
            )
        }
        AsonValue::Map(entries) => {
            let keys: Vec<_> = entries.iter().map(|(key, _)| key.clone()).collect();
            let values: Vec<_> = entries.iter().map(|(_, value)| value.clone()).collect();
            let (key_type, keys) = homogeneous_values(&keys)?;
            let (value_type, values) = homogeneous_values(&values)?;
            let entries = keys.into_iter().zip(values).collect();
            (
                Type::Map(false, Box::new(key_type), Box::new(value_type)),
                AsonbValue::Map(entries),
            )
        }
        AsonValue::Tuple(values) => {
            let mut types = Vec::with_capacity(values.len());
            let mut items = Vec::with_capacity(values.len());
            for value in values {
                let (ty, item) = to_asonb_value(value)?;
                types.push(ty);
                items.push(item);
            }
            (Type::Tuple(types), AsonbValue::Tuple(items))
        }
        AsonValue::Object(fields) => {
            let mut types = Vec::with_capacity(fields.len());
            let mut values = Vec::with_capacity(fields.len());
            for (name, value) in fields {
                let (ty, item) = to_asonb_value(value)?;
                types.push(Field::new(name, ty));
                values.push(item);
            }
            (
                Type::Object(String::new(), types),
                AsonbValue::Object(values),
            )
        }
        AsonValue::Enumeration {
            name,
            variant,
            payload,
        } => {
            let (variant_type, payload) = match payload {
                VariantValue::Unit => (Variant::Unit(variant.clone()), AsonbValue::Tuple(vec![])),
                VariantValue::Newtype(value) => {
                    let (ty, value) = to_asonb_value(value)?;
                    (Variant::Newtype(variant.clone(), ty), value)
                }
                VariantValue::Tuple(values) => {
                    let mut types = Vec::with_capacity(values.len());
                    let mut payload = Vec::with_capacity(values.len());
                    for value in values {
                        let (ty, item) = to_asonb_value(value)?;
                        types.push(ty);
                        payload.push(item);
                    }
                    (
                        Variant::Tuple(variant.clone(), types),
                        AsonbValue::Tuple(payload),
                    )
                }
                VariantValue::Struct(fields) => {
                    let mut types = Vec::with_capacity(fields.len());
                    let mut payload = Vec::with_capacity(fields.len());
                    for (field_name, value) in fields {
                        let (ty, item) = to_asonb_value(value)?;
                        types.push(Field::new(field_name, ty));
                        payload.push(item);
                    }
                    (
                        Variant::Struct(variant.clone(), types),
                        AsonbValue::Object(payload),
                    )
                }
            };
            (
                Type::Enum(name.clone(), vec![variant_type]),
                AsonbValue::Enum(0, Box::new(payload)),
            )
        }
        AsonValue::Option(None) => (
            option_type(Type::I32),
            AsonbValue::Enum(0, Box::new(AsonbValue::Tuple(vec![]))),
        ),
        AsonValue::Option(Some(value)) => {
            let (ty, value) = to_asonb_value(value)?;
            (option_type(ty), AsonbValue::Enum(1, Box::new(value)))
        }
    };
    Ok(converted)
}

fn option_type(some_type: Type) -> Type {
    Type::Enum(
        "Option".into(),
        vec![
            Variant::Unit("None".into()),
            Variant::Newtype("Some".into(), some_type),
        ],
    )
}

fn homogeneous_values(values: &[AsonValue]) -> Result<(Type, Vec<AsonbValue<'static>>)> {
    let Some(first) = values.first() else {
        return Ok((Type::I32, Vec::new()));
    };
    let (ty, first_value) = to_asonb_value(first)?;
    let mut converted = vec![first_value];
    for value in &values[1..] {
        let (next_type, next_value) = to_asonb_value(value)?;
        if next_type != ty {
            return Err(ConversionError(
                "ASONB collections require homogeneous element types.".into(),
            ));
        }
        converted.push(next_value);
    }
    Ok((ty, converted))
}

pub(crate) fn from_asonb_value(
    schema: &DocumentSchema,
    ty: &Type,
    value: &AsonbValue<'_>,
) -> Result<AsonValue> {
    let ty = resolve_type(schema, ty)?;
    let mismatch = || ConversionError(format!("ASONB value does not match schema type {ty:?}."));
    Ok(match (ty.clone(), value) {
        (Type::I8, AsonbValue::I8(value)) => AsonValue::I8(*value),
        (Type::I16, AsonbValue::I16(value)) => AsonValue::I16(*value),
        (Type::I32, AsonbValue::I32(value)) => AsonValue::I32(*value),
        (Type::I64, AsonbValue::I64(value)) => AsonValue::I64(*value),
        (Type::U8, AsonbValue::U8(value)) => AsonValue::U8(*value),
        (Type::U16, AsonbValue::U16(value)) => AsonValue::U16(*value),
        (Type::U32, AsonbValue::U32(value)) => AsonValue::U32(*value),
        (Type::U64, AsonbValue::U64(value)) => AsonValue::U64(*value),
        (Type::F32, AsonbValue::F32(value)) => AsonValue::F32(*value),
        (Type::F64, AsonbValue::F64(value)) => AsonValue::F64(*value),
        (Type::Bool, AsonbValue::Bool(value)) => AsonValue::Bool(*value),
        (Type::Char, AsonbValue::Char(value)) => AsonValue::Char(*value),
        (Type::String(_), AsonbValue::String(value)) => AsonValue::String(value.to_string()),
        (Type::DateTime, AsonbValue::DateTime(seconds)) => {
            let value = DateTime::<Utc>::from_timestamp(*seconds, 0)
                .ok_or_else(|| ConversionError("ASONB DateTime is out of range.".into()))?;
            AsonValue::DateTime(value.fixed_offset())
        }
        (Type::Bytes(_), AsonbValue::Bytes(value)) => AsonValue::Bytes(value.to_vec()),
        (Type::List(_, item_type), AsonbValue::List(values)) => AsonValue::List(
            values
                .iter()
                .map(|value| from_asonb_value(schema, &item_type, value))
                .collect::<Result<_>>()?,
        ),
        (Type::Map(_, key_type, value_type), AsonbValue::Map(entries)) => AsonValue::Map(
            entries
                .iter()
                .map(|(key, value)| {
                    Ok((
                        from_asonb_value(schema, &key_type, key)?,
                        from_asonb_value(schema, &value_type, value)?,
                    ))
                })
                .collect::<Result<_>>()?,
        ),
        (Type::Tuple(types), AsonbValue::Tuple(values)) if types.len() == values.len() => {
            AsonValue::Tuple(
                types
                    .iter()
                    .zip(values)
                    .map(|(ty, value)| from_asonb_value(schema, ty, value))
                    .collect::<Result<_>>()?,
            )
        }
        (Type::Object(_, fields), AsonbValue::Object(values)) if fields.len() == values.len() => {
            AsonValue::Object(
                fields
                    .iter()
                    .zip(values)
                    .map(|(field, value)| {
                        Ok((
                            field.name.clone(),
                            from_asonb_value(schema, &field.ty, value)?,
                        ))
                    })
                    .collect::<Result<_>>()?,
            )
        }
        (Type::Enum(name, variants), AsonbValue::Enum(index, payload)) => {
            let variant = variants.get(*index as usize).ok_or_else(mismatch)?;
            if name == "Option" {
                match (variant, payload.as_ref()) {
                    (Variant::Unit(variant), _) if variant == "None" => AsonValue::Option(None),
                    (Variant::Newtype(variant, ty), value) if variant == "Some" => {
                        AsonValue::Option(Some(Box::new(from_asonb_value(schema, ty, value)?)))
                    }
                    _ => return Err(mismatch()),
                }
            } else {
                let payload = match variant {
                    Variant::Unit(_) => VariantValue::Unit,
                    Variant::Newtype(_, ty) => {
                        VariantValue::Newtype(Box::new(from_asonb_value(schema, ty, payload)?))
                    }
                    Variant::Tuple(_, types) => {
                        let AsonbValue::Tuple(values) = payload.as_ref() else {
                            return Err(mismatch());
                        };
                        if types.len() != values.len() {
                            return Err(mismatch());
                        }
                        VariantValue::Tuple(
                            types
                                .iter()
                                .zip(values)
                                .map(|(ty, value)| from_asonb_value(schema, ty, value))
                                .collect::<Result<_>>()?,
                        )
                    }
                    Variant::Struct(_, fields) => {
                        let AsonbValue::Object(values) = payload.as_ref() else {
                            return Err(mismatch());
                        };
                        if fields.len() != values.len() {
                            return Err(mismatch());
                        }
                        VariantValue::Struct(
                            fields
                                .iter()
                                .zip(values)
                                .map(|(field, value)| {
                                    Ok((
                                        field.name.clone(),
                                        from_asonb_value(schema, &field.ty, value)?,
                                    ))
                                })
                                .collect::<Result<_>>()?,
                        )
                    }
                };
                AsonValue::Enumeration {
                    name,
                    variant: variant.name().into(),
                    payload,
                }
            }
        }
        _ => return Err(mismatch()),
    })
}

fn resolve_type(schema: &DocumentSchema, ty: &Type) -> Result<Type> {
    let mut resolved = ty.clone();
    for _ in 0..=schema.references.len() {
        let Type::Reference(index) = resolved else {
            return Ok(resolved);
        };
        resolved = schema
            .references
            .get(index as usize)
            .cloned()
            .ok_or_else(|| ConversionError("ASONB schema reference is out of range.".into()))?;
    }
    Err(ConversionError("ASONB schema contains a cycle.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ason_asonb_round_trip_with_rich_values() {
        let input = r#"{
    name: "Ada",
    values: [1, 2, 3],
    choice: Option::Some("yes"),
    action: Action::Move{x: 10 y: 20},
    mapping: ["first": 1 "second": 2]
}"#;
        let encoded = ason_to_asonb(input).unwrap();
        let output = asonb_to_ason(&encoded).unwrap();
        assert!(output.contains("Action::Move"));
        assert!(output.contains("Option::Some"));
        assert!(output.find("first").unwrap() < output.find("second").unwrap());
    }

    #[test]
    fn test_asonb_multichunk_list_streams_to_ason() {
        let schema = DocumentSchema::new(Type::List(Encoding::MultiChunk, Box::new(Type::I32)));
        let value = AsonbValue::List(vec![
            AsonbValue::I32(1),
            AsonbValue::I32(2),
            AsonbValue::I32(3),
        ]);
        let mut bytes = Vec::new();
        asonb::ser_value_to_writer(&schema, &value, &mut bytes).unwrap();

        let mut ason_output = Vec::new();
        asonb_reader_to_ason(bytes.as_slice(), &mut ason_output).unwrap();
        let ason_output = String::from_utf8(ason_output).unwrap();
        assert_eq!(
            ason::de_value_from_str(&ason_output).unwrap(),
            AsonValue::List(vec![
                AsonValue::I32(1),
                AsonValue::I32(2),
                AsonValue::I32(3)
            ])
        );
    }
}
