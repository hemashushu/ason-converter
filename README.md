# ASON Converter

ASON Converter provides the Rust library `ason-convert` and two command-line tools, `ason2b` and `ason2j`, for converting between ASON, ASONB (ASON's binary format), JSON, and JSONC.

<!-- @import "[TOC]" {cmd="toc" depthFrom=2 depthTo=4 orderedList=false} -->

<!-- code_chunk_output -->

- [1 Features](#1-features)
- [2 Limitations](#2-limitations)
- [3 Command-Line Tools](#3-command-line-tools)
  - [3.1 Installation](#31-installation)
  - [3.2 Commands](#32-commands)
    - [3.2.1 `ason2b`](#321-ason2b)
    - [3.2.2 `ason2j`](#322-ason2j)
    - [3.2.3 Exit Status](#323-exit-status)
- [4 Library Usage](#4-library-usage)
  - [4.1 Functions and Errors](#41-functions-and-errors)
  - [4.2 Streaming](#42-streaming)
- [5 Conversion Rules](#5-conversion-rules)
  - [5.1 Converting between ASON and ASONB](#51-converting-between-ason-and-asonb)
  - [5.2 Converting from ASON/ASONB to JSON](#52-converting-from-asonasonb-to-json)
    - [5.2.1 Numbers](#521-numbers)
    - [5.2.2 Maps](#522-maps)
    - [5.2.3 Enumerations](#523-enumerations)
    - [5.2.4 `Option`](#524-option)
    - [5.2.5 `Result`](#525-result)
  - [5.3 Converting from JSON to ASON/ASONB](#53-converting-from-json-to-asonasonb)
    - [5.3.1 Numbers](#531-numbers)
    - [5.3.2 Arrays](#532-arrays)
    - [5.3.3 Objects](#533-objects)
    - [5.3.4 Handling `null` Values](#534-handling-null-values)
    - [5.3.5 Numeric Type Promotion](#535-numeric-type-promotion)
    - [5.3.6 Promoting Objects to Maps](#536-promoting-objects-to-maps)
- [6 Building and Testing](#6-building-and-testing)
- [7 Links](#7-links)
- [8 License](#8-license)

<!-- /code_chunk_output -->

## 1 Features

- Converts between ASON, ASONB, and JSON.
- Reads [JSONC](https://jsonc.org/) with `//` and `/* ... */` comments and trailing commas; JSON output is standard JSON.
- Streams ASONB-to-ASON or ASONB-to-JSON conversions when the input meets the requirements below.
- Produces formatted ASON and JSON output.

## 2 Limitations

- The reader-based ASONB-to-ASON and ASONB-to-JSON conversions stream only a multi-chunk root List or a `Result::Ok` containing one. Other roots are read into memory. Conversions from ASON or JSON/JSONC read the entire input into memory.
- Comments and original formatting in ASON and JSONC input are discarded.
- ASONB stores DateTime values as whole-second UTC timestamps. An ASON-to-ASONB-to-ASON round trip loses the original UTC offset and leap-second representation. Direct ASON-to-JSON conversion preserves the offset; ASONB-to-JSON produces a UTC date-time string.
- ASON text does not preserve all ASONB schema or encoding information. An empty Map, an empty List with a non-default element type, or unused Enumeration variants may not survive an ASONB-to-ASON-to-ASONB round trip unchanged.
- Converting between JSON and ASON/ASONB may lose type information or numeric precision. A round trip may not reproduce the original document exactly.
- JSON cannot preserve distinctions such as Map versus List, Tuple versus List, an Enumeration's type name, or the `Some` wrapper of an `Option`.

## 3 Command-Line Tools

Two command-line tools are provided, each in its own package: `ason2b` converts between ASON and ASONB, and `ason2j` converts between ASON/ASONB and JSON.

### 3.1 Installation

Install the tools with Cargo:

```sh
cargo install ason2b
cargo install ason2j
```

To build the current workspace instead, install from a checkout:

```sh
git clone https://github.com/hemashushu/ason-converter.git
cd ason-converter
cargo install --locked --path crates/ason2b
cargo install --locked --path crates/ason2j
```

Both require a recent stable Rust toolchain.

### 3.2 Commands

Both commands accept an optional input file (`INPUT`) and `-o OUTPUT`. If `INPUT` is omitted, the command reads standard input; if `-o` is omitted, it writes standard output. Where a mode uses content-based input detection, recognized extensions are case-sensitive and take precedence over the ASONB header. Format conflicts are checked before an output file is opened, so a conflicting output is not created or truncated. An output file is created or truncated before conversion and may be empty or partial if conversion fails; use a different file from the input.

#### 3.2.1 `ason2b`

```text
ason2b [OPTIONS] [INPUT]
```

By default, `ason2b` converts ASON to ASONB. The `-r` option reverses the direction and converts ASONB to ASON. It recognizes `.ason` and `.asonb` input extensions; `.json` and `.jsonc` inputs are rejected. If the input has no recognized extension, an ASONB header identifies binary input; otherwise, the input is treated as ASON. The detected input format must match the selected direction.

- `-r`, `--reverse`: convert ASONB input to ASON.
- `-o OUTPUT`: write to a file. The file name must end with `.asonb` by default or `.ason` with `-r`; otherwise, conversion fails with exit status 2. This check does not apply when output is written to standard output.
- `--complete SHELL`: print a completion script for `bash`, `zsh`, `fish`, or `powershell`.

Examples:

```sh
# Convert input.ason to ASONB
ason2b input.ason -o output.asonb

# Convert input.asonb to ASON
ason2b -r input.asonb -o output.ason

# Convert ASON from standard input to ASONB and back to ASON
printf '%s\n' '{name: "Ada"}' | ason2b | ason2b -r
```

ASONB output cannot be written directly to a terminal; redirect it, pipe it, or use an output file. Input or output format conflicts and invalid documents return exit status 2. Unknown options return exit status 1.

For the detailed command reference in the checkout, see [`ason2b.txt`](./crates/ason2b/docs/ason2b.txt).

#### 3.2.2 `ason2j`

```text
ason2j [OPTIONS] [INPUT]
```

`ason2j` recognizes the case-sensitive input extensions `.ason`, `.asonb`, `.json`, and `.jsonc`. Without `-r`, it strictly converts ASON or ASONB to JSON. With `-r`, it converts JSON or JSONC to ASON, or to ASONB when `-b` is also specified. Output extensions never select or change the conversion direction or format. JSONC comments and trailing commas are discarded. This command does not convert between ASON and ASONB (use `ason2b` instead).

- `-r`, `--reverse`: require JSON or JSONC input and produce ASON. Input without a recognized extension is treated as JSON/JSONC in this mode.
- `-b`, `--binary`: produce ASONB instead of ASON; requires `-r`.
- `-o OUTPUT`: write to a file whose name matches the selected output format: `.json` by default, `.ason` with `-r`, or `.asonb` with `-r -b`. Any other extension (including `.jsonc`), a missing extension, or an extension with different casing is a format conflict (exit status 2). The output extension is not required when writing to standard output.
- `--complete SHELL`: print a completion script for `bash`, `zsh`, `fish`, or `powershell`.

Examples:

```sh
# Convert input.ason to JSON
ason2j input.ason -o output.json

# Convert input.asonb to JSON
ason2j input.asonb -o output.json

# Convert input.jsonc to ASON
ason2j -r input.jsonc -o output.ason

# Convert input.json to ASONB
ason2j -r -b input.json -o output.asonb

# Convert JSON from standard input to ASON
printf '%s\n' '{"name": "Ada"}' | ason2j -r

# Convert JSON from standard input to ASONB and back to ASON
printf '%s\n' '{"name": "Ada"}' | ason2j -r -b | ason2j
```

Without `-r`, input must be ASON or ASONB; a `.json` or `.jsonc` input is a format conflict. With `-r`, input must be JSON or JSONC; `.ason` and `.asonb` inputs are conflicts. A recognized input extension takes precedence over the ASONB header; without one, the default (non-reverse) mode uses the header to distinguish ASONB from ASON. `-b` without `-r` is an invalid-arguments error (exit status 1). ASONB output cannot be written directly to a terminal.

For the detailed command reference in the checkout, see [`ason2j.txt`](./crates/ason2j/docs/ason2j.txt).

#### 3.2.3 Exit Status

Both commands return these exit statuses:

| Status | Meaning |
|--------|---------|
| `0`    | Command succeeded, including conversion, help, version, or completion-script output. |
| `1`    | Invalid command-line arguments, such as an unknown option or a required-option error, or an unsupported completion shell. |
| `2`    | Invalid input, an input/output format conflict (including a mismatched output extension), or an attempt to write ASONB directly to a terminal. Format conflicts are detected before the output is opened. |
| `3`    | The input is valid, but its value cannot be represented in the selected output format. |
| `4`    | An I/O error while opening or reading input, or creating or writing output. |

## 4 Library Usage

Add the library to your project:

```sh
cargo add ason-convert
```

Or add the dependency to your project's `Cargo.toml`:

```toml
[dependencies]
ason-convert = "1.5.0"  # Use the latest version of the ason-convert library
```

For example, convert ASON to ASONB, back to ASON, and then to JSON:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = r#"{name: "Ada"}"#;

    let binary = ason_convert::ason_to_asonb(input)?;
    let ason = ason_convert::asonb_to_ason(&binary)?;
    let json = ason_convert::ason_to_json(&ason)?;

    println!("{json}");
    Ok(())
}
```

### 4.1 Functions and Errors

| Function | Input | Output |
|----------|-------|--------|
| `ason_to_asonb`        | ASON `&str`       | ASONB `Vec<u8>`         |
| `ason_to_json`         | ASON `&str`       | JSON `String`           |
| `asonb_to_ason`        | ASONB `&[u8]`     | ASON `String`           |
| `asonb_to_json`        | ASONB `&[u8]`     | JSON `String`           |
| `asonb_reader_to_ason` | ASONB `Read`      | ASON written to `Write` |
| `asonb_reader_to_json` | ASONB `Read`      | JSON written to `Write` |
| `json_to_ason`         | JSON/JSONC `&str` | ASON `String`           |
| `json_to_asonb`        | JSON/JSONC `&str` | ASONB `Vec<u8>`         |

The functions return `Result<_, ConversionError>`. `ConversionError` implements `std::error::Error` and can be propagated with `?`. `error.kind()` distinguishes `InvalidDocument`, `Unconvertible`, and `Io` via the non-exhaustive `ConversionErrorKind` enum. Include a wildcard arm when matching it. Error messages are for display, not stable programmatic matching.

### 4.2 Streaming

The reader functions accept a `Read` input and a mutable `Write` output. For example:

```rust
use ason_convert::asonb_reader_to_json;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = BufReader::new(File::open("input.asonb")?);
    let mut output = BufWriter::new(File::create("output.json")?);
    asonb_reader_to_json(input, &mut output)?;
    output.flush()?;
    Ok(())
}
```

A multi-chunk root List, or a multi-chunk List in the selected `Result::Ok` branch, is processed one item at a time; the schema and current item remain in memory. All other roots are decoded as a whole. The non-reader `asonb_to_ason` and `asonb_to_json` functions always decode the complete document in memory.

A streamed root List can end at EOF between chunks or at a terminator; a List inside `Result::Ok` requires a terminator. Bytes following a terminator are not checked or consumed. Other roots are checked for EOF, which can block on a live stream. Pass `&mut reader` if the reader must remain accessible afterwards.

The reader functions do not flush the writer. On error, the input may have been consumed and the output may contain a partial document. Multi-chunk input can be produced using the `asonb` crate's `list_stream_writer` or `result_stream_writer`.

## 5 Conversion Rules

### 5.1 Converting between ASON and ASONB

ASON and ASONB represent the same kinds of values. For details, see [Conversion to and from ASON](https://github.com/hemashushu/asonb#45-conversion-to-and-from-ason).

The predefined enumerations `Option` and `Result` are supported in both their qualified (`Option::Some(1)`, `Result::Err("failed")`) and shorthand (`Some(1)`, `None`, `Ok(1)`, `Err("failed")`) forms. ASON output always uses the shorthand form.

ASON text does not contain a complete ASONB schema, so ASON-to-ASONB conversion infers it from the values. Lists require one element type, and Maps require one key type and one value type. Numeric types are not promoted: for example, `[1, 2_i64]` cannot become an ASONB List. Objects in a collection must have the same fields in the same order. Types are unified recursively, with these inference rules:

- Enumeration values of the same type are merged, so `[Color::Red, Color::Green]` produces an enumeration type with both variants. A variant must have the same payload type everywhere it appears.
- An `Option` or `Result` branch can take its type from sibling values: `[None, Some("a")]` is a List of `Option<String>`, and `[Ok(1), Err("failed")]` is a List of `Result<i32, String>`. A type that cannot be inferred defaults to `i32`.
- Without context, `[]` is inferred as a List of `i32`. Consequently, an empty ASONB Map serialized as `[]` is inferred as a List when converted back.

### 5.2 Converting from ASON/ASONB to JSON

JSON supports fewer data types than ASON and ASONB. JSON has only six types:

- Null
- Boolean
- Number
- String
- Array
- Object

ASON and ASONB types without a direct JSON equivalent are mapped to compatible JSON types.

The table below summarizes how ASON/ASONB types map to JSON:

| ASON/ASONB Type                          | JSON Type                       |
|------------------------------------------|---------------------------------|
| Integer                                  | Number                          |
| Floating-point                           | Number                          |
| Boolean                                  | Boolean                         |
| Character                                | String                          |
| String                                   | String                          |
| DateTime                                 | String in RFC 3339 format       |
| Byte data                                | String in Base64 format         |
| List                                     | Array                           |
| Tuple                                    | Array                           |
| Object                                   | Object                          |
| Map                                      | Array                           |
| Enumeration                              | Object                          |
| `Some(value)` / `None`                   | `value` / `null`                |
| `Ok(value)` / `Err(error)`               | Object (as an Enumeration)      |

Byte data uses the standard Base64 alphabet and `=` padding defined in [RFC 4648, Section 4](https://datatracker.ietf.org/doc/html/rfc4648#section-4). For example, `h"00 ff"` becomes `"AP8="`.

#### 5.2.1 Numbers

- Integers outside the range [-(2^53 - 1), 2^53 - 1] may lose precision when parsed by JSON implementations that use double-precision floating-point numbers, such as JavaScript (see the [ECMAScript specification](https://tc39.es/ecma262/multipage/ecmascript-data-types-and-values.html#sec-ecmascript-language-types-number-type)).
- Special floating-point values, such as `NaN`, `Inf`, and `-Inf`, cannot be represented in JSON and conversion fails.
- JSON numbers do not preserve whether a value was originally `f32` or `f64`.

#### 5.2.2 Maps

Maps become JSON arrays of key-value pair objects. Each object has `key` and `value` fields, and entries remain in their original order:

```text
[
  {
    "key": KEY1,
    "value": VALUE1
  },
  {
    "key": KEY2,
    "value": VALUE2
  }
]
```

For example, this ASON map:

```text
[
    "red": "#FF0000",
    "green": "#00FF00"
]
```

becomes:

```json
[
    {
        "key": "red",
        "value": "#FF0000"
    },
    {
        "key": "green",
        "value": "#00FF00"
    }
]
```

#### 5.2.3 Enumerations

Enumerations use an adjacent-tagged JSON representation. Each value becomes an object with a `variant` field and, when the variant has a payload, a `value` field:

```text
{
  "variant": VARIANT_NAME,
  "value": VALUE_IF_PRESENT
}
```

For example, consider this enumeration:

```rust
enum Action {
    Quit,
    Say(String),
    Take(String, i32),
    Move { x: i32, y: i32 },
}
```

Its variants map to JSON as follows:

- `Action::Quit`:

```json
{
  "variant": "Quit"
}
```

- `Action::Say("Hello")`:

```json
{
  "variant": "Say",
  "value": "Hello"
}
```

- `Action::Take("Item", 42)`:

```json
{
  "variant": "Take",
  "value": ["Item", 42]
}
```

- `Action::Move { x: 10, y: 20 }`:

```json
{
  "variant": "Move",
  "value": {
    "x": 10,
    "y": 20
  }
}
```

The JSON representation does not include the enumeration's type name (`Action` here).

Variants whose payload is a single Tuple or Object use the same array or object representation as other tuple-like or struct-like variants.

#### 5.2.4 `Option`

Options map to `null` for `None` and to the contained value for `Some(value)`:

```text
None -> null
Some(VALUE) -> VALUE
```

#### 5.2.5 `Result`

Results are converted like other enumerations:

```text
Ok(VALUE) -> {"variant": "Ok", "value": VALUE}
Err(ERROR) -> {"variant": "Err", "value": ERROR}
```

### 5.3 Converting from JSON to ASON/ASONB

JSON-to-ASON/ASONB conversion considers the entire document because context affects how values such as `null` are interpreted.

The table below summarizes how JSON types map to ASON/ASONB:

| JSON Type | ASON/ASONB Type                                                                         |
|-----------|-----------------------------------------------------------------------------------------|
| Null      | `None`, with an inferred or default `Option` type                                       |
| Number    | `i32`, `i64`, `u64`, or `f64`, depending on the value                                   |
| Boolean   | Boolean                                                                                 |
| String    | String                                                                                  |
| Array     | List if the elements can be unified to one ASON type; otherwise, an error               |
| Object    | Object or Map, depending on its keys, values, and context                              |

#### 5.3.1 Numbers

Numbers with a decimal point (`.`) or exponent (`e` or `E`), and the token `-0`, are converted to `f64`. Other numbers are assigned a type based on their range:

- `i32`: if its value is within `[i32::MIN, i32::MAX]`
- `i64`: if its value is within `[i64::MIN, i64::MAX]`
- `u64`: if its value is larger than `i64::MAX` and less than or equal to `u64::MAX`
- `f64`: if an integer is outside the ranges of `i32`, `i64`, and `u64`, but is representable as a finite `f64`.

Values outside the parser's finite floating-point range, such as `1e309`, are rejected. Conversion to `f64` may lose precision.

#### 5.3.2 Arrays

An array becomes a List if its elements can be unified into one ASON type after inference and promotion. Otherwise, conversion fails.

For example:

```json
[1, 2, 3]
```

becomes a List of integers. This array cannot be converted because its elements have different types:

```json
[1, "two", true]
```

An array's type depends on its elements, not its length. An empty array defaults to a List of `i32` when its type cannot be inferred from context or sibling elements. See the following sections for arrays containing objects or `null`.

For nested arrays, each inner array must be homogeneous, and all inner arrays must be able to share an ASON type after promotion. For example:

```json
[[1, 2], [3, 4]]
```

becomes a List of Lists of integers. These arrays cannot be converted: the first has an inner array with mixed element types, and the second has inner arrays with different ASON types.

```json
[[1, "two"], [3, 4]]
```

```json
[[1], ["one"]]
```

When possible, an empty nested array takes its type from sibling arrays. Otherwise, it defaults to a List of `i32`. For example:

```json
[[], ["foo", "bar"]]
```

becomes a List of Lists of String; the empty inner array is inferred as a List of `String`.

#### 5.3.3 Objects

When converted on its own, a non-empty object becomes an ASON/ASONB Object if all its keys are valid identifiers.

For example:

```json
{"id": 1, "name": "Alice"}
```

becomes an ASON/ASONB Object.

An identifier starts with an ASCII letter, `_`, or a Unicode scalar at or above U+00A0; subsequent characters may also be ASCII digits. The ASON keywords `true`, `false`, `NaN`, `NaN_f32`, `NaN_f64`, `Inf`, `Inf_f32`, and `Inf_f64` are reserved and are not treated as valid keys for an Object. The variant shorthands `Some`, `None`, `Ok`, and `Err` are ordinary identifiers and can be Object keys.

If any key is not a valid identifier, the object becomes a Map only if all its values have a common ASON type after promotion. Objects in a collection may be promoted to Maps to match other elements; see [Promoting Objects to Maps](#536-promoting-objects-to-maps).

For example, the following object can be converted to a Map:

```json
{"asonb-derive": "3.0", "proc-macro2": "1.0"}
```

If a JSON object cannot be converted to an ASON/ASONB Object or Map, conversion fails.

JSON objects with duplicate keys are rejected. Strings containing unpaired surrogate escapes are also rejected because ASON strings require valid Unicode scalar values.

For an array of objects with valid identifier keys, conversion collects keys in first-seen order and infers a shared schema. Every field is an `Option`, including fields present in every row. Missing or null fields become `None`, and non-null fields become `Some(value)`; incompatible values cause conversion to fail. If the array contains a null row, it becomes `None`, while the non-null rows become `Some(object)`.

For example, `[{"id": 1, "name": "Ada"}, {"id": 2}]` becomes:

```text
[
    {id: Some(1), name: Some("Ada")}
    {id: Some(2), name: None}
]
```

If context does not provide keys for an empty object, conversion fails because ASON/ASONB Objects must have keys.

#### 5.3.4 Handling `null` Values

A standalone `null` becomes `Option::None` with type `Option<i32>`. A `null` object property uses the same default unless its type can be inferred from context. In an ordinary object, only null-valued properties become Options; other properties are converted independently.

When `null` appears in an array, in Map values, or for a key shared across an array of objects, it becomes `Option::None`; the corresponding non-null values become `Option::Some(value)`. The non-null values determine the Option's type. If all values are null, the type defaults to `Option<i32>`. Conversion fails if the non-null values have incompatible ASON types after promotion.

#### 5.3.5 Numeric Type Promotion

When finding a common ASON type for numeric array elements, Map values, or values for the same key across an array of objects, use these promotion rules:

- `i32` and `i64` values are promoted to `i64`.
- Without floating-point values, signed integers (`i32` or `i64`) mixed with `u64` values are promoted to `u64` if all values are non-negative; otherwise, conversion fails.
- Integer values mixed with floating-point values are promoted to `f64`.
- Other mixtures that cannot be represented by one ASON type cause conversion to fail.

Promotion to `f64` may lose integer precision for large values.

#### 5.3.6 Promoting Objects to Maps

When a collection needs a common ASON type and contains both Objects and Maps, an Object is promoted to a Map if the values can share one ASON type. Each Object field name becomes a String key, and all Object field values and Map values are unified, including numeric promotion where applicable. If the values cannot be unified, conversion fails. This promotion occurs only when a Map is already present; Objects are not converted to Maps solely because they appear in a collection.

For example, the first value below is initially an Object and the second is a Map because its key is not a valid identifier:

```json
[{"id": 1}, {"invalid-key": 2147483648}]
```

The Object is promoted to a Map, and `1` is promoted to `i64`. The result is a List of `Map<String, i64>` values.

## 6 Building and Testing

From the workspace root:

```sh
cargo build --workspace --locked
cargo test --workspace --all-features --locked
```

## 7 Links

- [ASON Specification](https://github.com/hemashushu/ason/blob/main/docs/specification.md)
- [ASONB Specification](https://github.com/hemashushu/asonb/blob/main/README.md)
- [ASON Formatter (asonfmt)](https://github.com/hemashushu/ason-formatter)
- [Library API documentation](https://docs.rs/ason-convert)

## 8 License

This project is licensed under the MPL 2.0 License with additional terms. See [LICENSE](./LICENSE) and [LICENSE.additional](./LICENSE.additional).
