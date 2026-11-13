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
use asonb::{
    DocumentSchema, Encoding, Field, Type, Value as AsonbValue, Variant,
    VariantValue as AsonbVariantValue,
};
use chrono::{DateTime, Utc};

#[derive(Debug)]
/// An error raised while parsing or converting an ASON or ASONB value.
///
/// The message describes the underlying parse, schema, encoding, or I/O failure.
pub struct ConversionError(
    #[doc = "The underlying parser, schema, serialization, or I/O error message."] pub(crate) String,
);

impl fmt::Display for ConversionError {
    /// Writes the underlying conversion error message without additional decoration.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ConversionError {}

impl From<ason::AsonError> for ConversionError {
    /// Preserves the parser's human-readable message in the public error type.
    fn from(error: ason::AsonError) -> Self {
        Self(error.to_string())
    }
}

impl From<asonb::AsonbError> for ConversionError {
    /// Converts an ASONB parsing or serialization error into the shared error type.
    fn from(error: asonb::AsonbError) -> Self {
        Self(error.to_string())
    }
}

impl From<serde_json::Error> for ConversionError {
    /// Converts a JSON parsing or formatting error into the shared error type.
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<std::io::Error> for ConversionError {
    /// Converts a stream read or write failure into the shared error type.
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

pub(crate) type Result<T> = std::result::Result<T, ConversionError>;

/// Parses an ASON value and encodes it as a complete ASONB document.
///
/// The returned bytes include the document schema and binary header. ASONB requires each
/// collection to have one element type, so heterogeneous lists or map keys/values return an
/// error. Empty collections use `i32` as their inferred element type because no value is
/// available to infer a more specific type.
///
/// # Errors
/// Returns an error if the input is invalid ASON, a collection has incompatible element types,
/// or ASONB serialization fails.
pub fn ason_to_asonb(input: &str) -> Result<Vec<u8>> {
    let value = ason::de_value_from_str(input)?;
    let (root_type, binary_value) = to_asonb_value(&value)?;
    let schema = DocumentSchema::new(root_type);
    let mut output = Vec::new();
    asonb::ser_value_to_writer(&schema, &binary_value, &mut output)?;
    Ok(output)
}

/// Decodes one complete ASONB document and serializes its root value as ASON text.
///
/// Unlike [`asonb_reader_to_ason`], this convenience function buffers the decoded root value
/// before producing the returned string.
///
/// # Errors
/// Returns an error if the bytes do not contain a valid ASONB document or if its schema and
/// value disagree.
pub fn asonb_to_ason(input: &[u8]) -> Result<String> {
    let (schema, value) = asonb::de_value_from_reader(Cursor::new(input))?;
    let value = from_asonb_value(&schema, &schema.root, &value)?;
    Ok(ason::ser_value_to_string(&value)?)
}

/// Converts an ASONB document from a reader directly into ASON text on a writer.
///
/// A multi-chunk root List, or a multi-chunk List inside a root `Result::Ok`, is written one item
/// at a time, keeping memory use bounded by the current item. Other root values are decoded as a
/// single value. The input and output streams remain owned by the caller.
///
/// # Errors
/// Returns an error for invalid ASONB data, an absent root value, a schema/value mismatch, or an
/// input/output failure.
pub fn asonb_reader_to_ason<R: Read, W: Write>(input: R, output: &mut W) -> Result<()> {
    let mut document = asonb::value_document_reader(input)?;
    let schema = document.schema().clone();
    let root_type = document.root_type().clone();
    if let Some(item_type) = document.item_type().cloned() {
        // Keep multi-chunk lists streaming instead of collecting the full list in memory.
        let mut writer = if document.is_streaming_result() {
            ason::result_stream_writer::<AsonValue, _>(output).start_ok()?
        } else {
            let mut writer = ason::list_stream_writer::<AsonValue, _>(output);
            writer.start_list()?;
            writer
        };
        for value in document {
            let value = value?;
            let value = from_asonb_value(&schema, &item_type, &value)?;
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

/// Placeholder for a type that no value determines, such as the element type of an empty List,
/// the payload of `None`, or the unselected branch of a `Result`.
///
/// It never reaches the written schema: [`finalize_type`] replaces it with `i32`. A reference index
/// is used because ASONB has no "unknown" type and real references are never produced here.
const UNKNOWN: Type = Type::Reference(u32::MAX);

/// Infers the ASONB schema for an ASON value and encodes the value with it.
///
/// ASON text does not carry the homogeneous collection schema required by ASONB, so the type is
/// inferred first and the value is encoded afterwards; variant indices depend on the complete,
/// unified Enumeration schema.
fn to_asonb_value(value: &AsonValue) -> Result<(Type, AsonbValue<'static>)> {
    let ty = finalize_type(infer_type(value)?);
    let value = encode_value(value, &ty)?;
    Ok((ty, value))
}

/// Infers the ASONB type of one ASON value, possibly containing [`UNKNOWN`] placeholders.
fn infer_type(value: &AsonValue) -> Result<Type> {
    Ok(match value {
        AsonValue::I8(_) => Type::I8,
        AsonValue::I16(_) => Type::I16,
        AsonValue::I32(_) => Type::I32,
        AsonValue::I64(_) => Type::I64,
        AsonValue::U8(_) => Type::U8,
        AsonValue::U16(_) => Type::U16,
        AsonValue::U32(_) => Type::U32,
        AsonValue::U64(_) => Type::U64,
        AsonValue::F32(_) => Type::F32,
        AsonValue::F64(_) => Type::F64,
        AsonValue::Bool(_) => Type::Bool,
        AsonValue::Char(_) => Type::Char,
        AsonValue::String(_) => Type::String(Encoding::Regular),
        AsonValue::DateTime(_) => Type::DateTime,
        AsonValue::Bytes(_) => Type::Bytes(Encoding::Regular),
        AsonValue::List(values) => Type::List(
            Encoding::Regular,
            Box::new(infer_common_type(values.iter())?),
        ),
        AsonValue::Map(entries) => Type::Map(
            false,
            Box::new(infer_common_type(entries.iter().map(|(key, _)| key))?),
            Box::new(infer_common_type(entries.iter().map(|(_, value)| value))?),
        ),
        AsonValue::Tuple(values) => {
            Type::Tuple(values.iter().map(infer_type).collect::<Result<_>>()?)
        }
        AsonValue::Object(fields) => Type::Object(infer_fields(fields)?),
        AsonValue::Enumeration {
            name,
            variant,
            payload,
        } if name == "Option" => match (variant.as_str(), payload) {
            ("None", VariantValue::Unit) => option_type(UNKNOWN),
            ("Some", VariantValue::Newtype(value)) => option_type(infer_type(value)?),
            _ => {
                return Err(ConversionError(
                    "Expect \"Option::None\" or \"Option::Some(...)\".".into(),
                ));
            }
        },
        AsonValue::Enumeration {
            name,
            variant,
            payload,
        } if name == "Result" => {
            let VariantValue::Newtype(value) = payload else {
                return Err(ConversionError(format!(
                    "\"Result::{variant}\" must have exactly one value."
                )));
            };
            let value_type = infer_type(value)?;
            match variant.as_str() {
                "Ok" => result_type(value_type, UNKNOWN),
                "Err" => result_type(UNKNOWN, value_type),
                _ => {
                    return Err(ConversionError(
                        "Expect \"Result::Ok(...)\" or \"Result::Err(...)\".".into(),
                    ));
                }
            }
        }
        AsonValue::Enumeration {
            name,
            variant,
            payload,
        } => {
            let variant = match payload {
                VariantValue::Unit => Variant::Unit(variant.clone()),
                VariantValue::Newtype(value) => {
                    Variant::Newtype(variant.clone(), infer_type(value)?)
                }
                VariantValue::Tuple(values) => Variant::Tuple(
                    variant.clone(),
                    values.iter().map(infer_type).collect::<Result<_>>()?,
                ),
                VariantValue::Struct(fields) => {
                    Variant::Struct(variant.clone(), infer_fields(fields)?)
                }
            };
            Type::Enum(name.clone(), vec![variant])
        }
    })
}

/// Infers the field types of an Object or object-like variant, in field order.
fn infer_fields(fields: &[(String, AsonValue)]) -> Result<Vec<Field>> {
    fields
        .iter()
        .map(|(name, value)| Ok(Field::new(name, infer_type(value)?)))
        .collect()
}

/// Infers the single type shared by collection elements; no elements yield [`UNKNOWN`].
fn infer_common_type<'a>(mut values: impl Iterator<Item = &'a AsonValue>) -> Result<Type> {
    values.try_fold(UNKNOWN, |ty, value| unify_types(ty, infer_type(value)?))
}

/// Builds the schema of the predefined `Option` Enumeration.
fn option_type(some_type: Type) -> Type {
    Type::Enum(
        "Option".into(),
        vec![
            Variant::Unit("None".into()),
            Variant::Newtype("Some".into(), some_type),
        ],
    )
}

/// Builds the schema of the predefined `Result` Enumeration.
fn result_type(ok_type: Type, err_type: Type) -> Type {
    Type::Enum(
        "Result".into(),
        vec![
            Variant::Newtype("Ok".into(), ok_type),
            Variant::Newtype("Err".into(), err_type),
        ],
    )
}

/// Combines two inferred types into one, as required for ASONB collection elements.
///
/// [`UNKNOWN`] matches any type, and Enumerations of the same name merge their variants by name.
/// Other differences are rejected rather than silently promoting values, which keeps the
/// conversion faithful to the input's declared ASON types.
fn unify_types(left: Type, right: Type) -> Result<Type> {
    if left == right || right == UNKNOWN {
        return Ok(left);
    }
    if left == UNKNOWN {
        return Ok(right);
    }
    let mismatch =
        || ConversionError("ASONB collections require homogeneous element types.".into());
    Ok(match (left, right) {
        (Type::List(_, left), Type::List(_, right)) => {
            Type::List(Encoding::Regular, Box::new(unify_types(*left, *right)?))
        }
        (Type::Map(_, left_key, left_value), Type::Map(_, right_key, right_value)) => Type::Map(
            false,
            Box::new(unify_types(*left_key, *right_key)?),
            Box::new(unify_types(*left_value, *right_value)?),
        ),
        (Type::Tuple(left), Type::Tuple(right)) if left.len() == right.len() => Type::Tuple(
            left.into_iter()
                .zip(right)
                .map(|(left, right)| unify_types(left, right))
                .collect::<Result<_>>()?,
        ),
        (Type::Object(left), Type::Object(right)) => {
            Type::Object(unify_fields(left, right).ok_or_else(mismatch)??)
        }
        (Type::Enum(name, mut variants), Type::Enum(right_name, right_variants))
            if name == right_name =>
        {
            for right in right_variants {
                let Some(index) = variants
                    .iter()
                    .position(|variant| variant.name() == right.name())
                else {
                    variants.push(right);
                    continue;
                };
                let left = variants[index].clone();
                variants[index] = match (left, right) {
                    (Variant::Unit(name), Variant::Unit(_)) => Variant::Unit(name),
                    (Variant::Newtype(name, left), Variant::Newtype(_, right)) => {
                        Variant::Newtype(name, unify_types(left, right)?)
                    }
                    (Variant::Tuple(name, left), Variant::Tuple(_, right))
                        if left.len() == right.len() =>
                    {
                        Variant::Tuple(
                            name,
                            left.into_iter()
                                .zip(right)
                                .map(|(left, right)| unify_types(left, right))
                                .collect::<Result<_>>()?,
                        )
                    }
                    (Variant::Struct(name, left), Variant::Struct(_, right)) => {
                        Variant::Struct(name, unify_fields(left, right).ok_or_else(mismatch)??)
                    }
                    _ => return Err(mismatch()),
                };
            }
            Type::Enum(name, variants)
        }
        _ => return Err(mismatch()),
    })
}

/// Unifies fields with the same names in the same order; `None` means the names differ.
fn unify_fields(left: Vec<Field>, right: Vec<Field>) -> Option<Result<Vec<Field>>> {
    let same_names = left.len() == right.len()
        && left
            .iter()
            .zip(&right)
            .all(|(left, right)| left.name == right.name);
    same_names.then(|| {
        left.into_iter()
            .zip(right)
            .map(|(left, right)| Ok(Field::new(&left.name, unify_types(left.ty, right.ty)?)))
            .collect()
    })
}

/// Replaces every [`UNKNOWN`] placeholder with `i32`, the documented fallback type.
fn finalize_type(ty: Type) -> Type {
    let finalize_fields = |fields: Vec<Field>| {
        fields
            .into_iter()
            .map(|field| Field::new(&field.name, finalize_type(field.ty)))
            .collect()
    };
    match ty {
        UNKNOWN => Type::I32,
        Type::List(encoding, item) => Type::List(encoding, Box::new(finalize_type(*item))),
        Type::Map(long, key, value) => Type::Map(
            long,
            Box::new(finalize_type(*key)),
            Box::new(finalize_type(*value)),
        ),
        Type::Tuple(types) => Type::Tuple(types.into_iter().map(finalize_type).collect()),
        Type::Object(fields) => Type::Object(finalize_fields(fields)),
        Type::Enum(name, variants) => Type::Enum(
            name,
            variants
                .into_iter()
                .map(|variant| match variant {
                    Variant::Unit(name) => Variant::Unit(name),
                    Variant::Newtype(name, ty) => Variant::Newtype(name, finalize_type(ty)),
                    Variant::Tuple(name, types) => {
                        Variant::Tuple(name, types.into_iter().map(finalize_type).collect())
                    }
                    Variant::Struct(name, fields) => Variant::Struct(name, finalize_fields(fields)),
                })
                .collect(),
        ),
        ty => ty,
    }
}

/// Encodes an ASON value using the complete schema inferred for it.
///
/// Enumeration variants are located by name because unification may have merged several
/// variants into one schema.
fn encode_value(value: &AsonValue, ty: &Type) -> Result<AsonbValue<'static>> {
    let mismatch = || ConversionError(format!("ASON value does not match ASONB type {ty:?}."));
    Ok(match (value, ty) {
        (AsonValue::I8(value), _) => AsonbValue::I8(*value),
        (AsonValue::I16(value), _) => AsonbValue::I16(*value),
        (AsonValue::I32(value), _) => AsonbValue::I32(*value),
        (AsonValue::I64(value), _) => AsonbValue::I64(*value),
        (AsonValue::U8(value), _) => AsonbValue::U8(*value),
        (AsonValue::U16(value), _) => AsonbValue::U16(*value),
        (AsonValue::U32(value), _) => AsonbValue::U32(*value),
        (AsonValue::U64(value), _) => AsonbValue::U64(*value),
        (AsonValue::F32(value), _) => AsonbValue::F32(*value),
        (AsonValue::F64(value), _) => AsonbValue::F64(*value),
        (AsonValue::Bool(value), _) => AsonbValue::Bool(*value),
        (AsonValue::Char(value), _) => AsonbValue::Char(*value),
        (AsonValue::String(value), _) => AsonbValue::String(Cow::Owned(value.clone())),
        (AsonValue::DateTime(value), _) => AsonbValue::DateTime(value.timestamp()),
        (AsonValue::Bytes(value), _) => AsonbValue::Bytes(Cow::Owned(value.clone())),
        (AsonValue::List(values), Type::List(_, item)) => AsonbValue::List(
            values
                .iter()
                .map(|value| encode_value(value, item))
                .collect::<Result<_>>()?,
        ),
        (AsonValue::Map(entries), Type::Map(_, key_type, value_type)) => AsonbValue::Map(
            entries
                .iter()
                .map(|(key, value)| {
                    Ok((
                        encode_value(key, key_type)?,
                        encode_value(value, value_type)?,
                    ))
                })
                .collect::<Result<_>>()?,
        ),
        (AsonValue::Tuple(values), Type::Tuple(types)) if values.len() == types.len() => {
            AsonbValue::Tuple(
                values
                    .iter()
                    .zip(types)
                    .map(|(value, ty)| encode_value(value, ty))
                    .collect::<Result<_>>()?,
            )
        }
        (AsonValue::Object(values), Type::Object(fields)) if values.len() == fields.len() => {
            AsonbValue::Object(encode_fields(values, fields)?)
        }
        (
            AsonValue::Enumeration {
                variant, payload, ..
            },
            Type::Enum(_, variants),
        ) => {
            let (index, variant) = find_variant(variants, variant).ok_or_else(mismatch)?;
            let payload = match (payload, variant) {
                (VariantValue::Unit, Variant::Unit(_)) => AsonbVariantValue::Unit,
                (VariantValue::Newtype(value), Variant::Newtype(_, ty)) => {
                    AsonbVariantValue::Newtype(Box::new(encode_value(value, ty)?))
                }
                (VariantValue::Tuple(values), Variant::Tuple(_, types))
                    if values.len() == types.len() =>
                {
                    AsonbVariantValue::Tuple(
                        values
                            .iter()
                            .zip(types)
                            .map(|(value, ty)| encode_value(value, ty))
                            .collect::<Result<_>>()?,
                    )
                }
                (VariantValue::Struct(values), Variant::Struct(_, fields))
                    if values.len() == fields.len() =>
                {
                    AsonbVariantValue::Struct(encode_fields(values, fields)?)
                }
                _ => return Err(mismatch()),
            };
            AsonbValue::Enumeration { index, payload }
        }
        _ => return Err(mismatch()),
    })
}

/// Encodes Object or object-like variant fields in schema order.
fn encode_fields(
    values: &[(String, AsonValue)],
    fields: &[Field],
) -> Result<Vec<AsonbValue<'static>>> {
    values
        .iter()
        .zip(fields)
        .map(|((_, value), field)| encode_value(value, &field.ty))
        .collect()
}

/// Finds a variant and its wire index by name.
fn find_variant<'a>(variants: &'a [Variant], name: &str) -> Option<(u32, &'a Variant)> {
    variants
        .iter()
        .enumerate()
        .find(|(_, variant)| variant.name() == name)
        .map(|(index, variant)| (index as u32, variant))
}

/// Reconstructs an ASON value using its schema-resolved ASONB type.
///
/// This function checks container lengths and enum payload shapes as it descends. Those checks
/// are important because binary values alone do not describe object field names or tuple item
/// types; that information lives in the document schema.
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
        (Type::Object(fields), AsonbValue::Object(values)) if fields.len() == values.len() => {
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
        (Type::Enum(name, variants), AsonbValue::Enumeration { index, payload }) => {
            let variant = variants.get(*index as usize).ok_or_else(mismatch)?;
            let payload = match (variant, payload) {
                (Variant::Unit(_), AsonbVariantValue::Unit) => VariantValue::Unit,
                (Variant::Newtype(_, ty), AsonbVariantValue::Newtype(value)) => {
                    VariantValue::Newtype(Box::new(from_asonb_value(schema, ty, value)?))
                }
                (Variant::Tuple(_, types), AsonbVariantValue::Tuple(values)) => {
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
                (Variant::Struct(_, fields), AsonbVariantValue::Struct(values)) => {
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
                _ => return Err(mismatch()),
            };
            AsonValue::Enumeration {
                name,
                variant: variant.name().into(),
                payload,
            }
        }
        _ => return Err(mismatch()),
    })
}

/// Follows schema references until a concrete type is found.
///
/// The bounded loop allows valid chains of references while preventing malformed cyclic schemas
/// from causing an infinite loop.
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
    /// Checks that representative nested values survive an ASON-to-ASONB-to-ASON round trip.
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
        assert!(output.contains("Some(\"yes\")"));
        assert!(output.find("first").unwrap() < output.find("second").unwrap());
    }

    #[test]
    /// Checks predefined Enumerations, their shorthands, and variant merging in collections.
    fn test_ason_asonb_predefined_enumerations_and_shorthands() {
        let input = r#"{
    nested: Some(Ok([11, 13, 17]))
    qualified: Result::Ok(Option::Some(11))
    results: [Ok(1), Err("failed"), Result::Ok(2)]
    options: [None, Some("a"), Option::None]
    colors: [Color::Red, Color::Rgb(255, 127, 63), Color::Red]
    empty: [[], [Err(1)]]
}"#;
        let encoded = ason_to_asonb(input).unwrap();
        let (schema, _) = asonb::de_value_from_slice(&encoded).unwrap();
        let Type::Object(fields) = &schema.root else {
            panic!("expected root Object schema");
        };
        let Type::List(_, result_type) = &fields[2].ty else {
            panic!("expected List of Result");
        };
        assert_eq!(
            **result_type,
            result_type_of(Type::I32, Type::String(Encoding::Regular))
        );
        let Type::List(_, color_type) = &fields[4].ty else {
            panic!("expected List of Color");
        };
        assert!(matches!(color_type.as_ref(), Type::Enum(_, variants) if variants.len() == 2));
        let Type::List(_, empty_type) = &fields[5].ty else {
            panic!("expected List of Lists");
        };
        assert_eq!(
            **empty_type,
            Type::List(
                Encoding::Regular,
                Box::new(result_type_of(Type::I32, Type::I32))
            )
        );

        let output = asonb_to_ason(&encoded).unwrap();
        assert_eq!(
            ason::de_value_from_str(&output).unwrap(),
            ason::de_value_from_str(input).unwrap()
        );
        assert!(output.contains("Some(Ok("));
        assert!(output.contains("Ok(Some(11))"));
        assert!(!output.contains("Option::") && !output.contains("Result::"));

        assert!(ason_to_asonb("[Ok(1), Ok(\"two\")]").is_err());
        assert!(ason_to_asonb("[Color::Red, Color::Red(1)]").is_err());
    }

    /// Builds the expected ASONB schema of `Result<T, E>`.
    fn result_type_of(ok: Type, err: Type) -> Type {
        Type::Enum(
            "Result".into(),
            vec![
                Variant::Newtype("Ok".into(), ok),
                Variant::Newtype("Err".into(), err),
            ],
        )
    }

    #[test]
    /// Confirms that `Result::Ok` with a multi-chunk List streams, and `Err` is written whole.
    fn test_asonb_result_stream_to_ason() {
        let mut bytes = Vec::new();
        let mut writer = asonb::result_stream_writer::<i32, String, _>(&mut bytes)
            .start_ok()
            .unwrap();
        writer.writes(&[11, 13, 17]).unwrap();
        writer.end_list().unwrap();

        let mut output = Vec::new();
        asonb_reader_to_ason(bytes.as_slice(), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("Ok(["));
        assert_eq!(
            ason::de_from_str::<std::result::Result<Vec<i32>, String>>(&output).unwrap(),
            Ok(vec![11, 13, 17])
        );

        // The List inside a Result must be terminated explicitly.
        let mut output = Vec::new();
        assert!(asonb_reader_to_ason(&bytes[..bytes.len() - 4], &mut output).is_err());

        let mut bytes = Vec::new();
        asonb::result_stream_writer::<i32, String, _>(&mut bytes)
            .write_err(&"failed".to_owned())
            .unwrap();
        let mut output = Vec::new();
        asonb_reader_to_ason(bytes.as_slice(), &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), r#"Err("failed")"#);
    }

    #[test]
    /// Confirms that a multi-chunk ASONB list is converted through the streaming path.
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
