# ASON Converter

ASON Converter is a tool for converting between ASON, ASONB (the binary format of ASON), and JSON.

<!-- @import "[TOC]" {cmd="toc" depthFrom=2 depthTo=4 orderedList=false} -->

<!-- code_chunk_output -->

- [1 Features](#1-features)
- [2 Limitations](#2-limitations)
- [3 Conversion Rules](#3-conversion-rules)
  - [3.1 Converting between ASON and ASONB](#31-converting-between-ason-and-asonb)
  - [3.2 Converting ASON/ASONB to JSON](#32-converting-asonasonb-to-json)
    - [3.2.1 Number](#321-number)
    - [3.2.2 Map](#322-map)
    - [3.2.3 Enumeration](#323-enumeration)
    - [3.2.4 `Option`](#324-option)
  - [3.3 Converting from JSON to ASON/ASONB](#33-converting-from-json-to-asonasonb)
    - [3.3.1 Numbers](#331-numbers)
    - [3.3.2 Array](#332-array)
    - [3.3.3 Object](#333-object)
    - [3.3.4 Handling `null` Values](#334-handling-null-values)
    - [3.3.5 Numeric Type Promotion](#335-numeric-type-promotion)
- [4. Installation](#4-installation)
- [5. Usage](#5-usage)
  - [`ason2b`](#ason2b)
  - [`ason2j`](#ason2j)
- [6. Links](#6-links)
- [7. License](#7-license)

<!-- /code_chunk_output -->

## 1 Features

- Converts between ASON, ASONB, and JSON.
- Supports reading [JSONC](https://jsonc.org/) (JSON with comments).
- Supports streaming conversion from ASONB to ASON or JSON.
- Produces well-formatted ASON and JSON output.

## 2 Limitations

- Streaming conversion from ASONB to ASON or JSON is available only when the root value is a multi-chunk List. Streaming conversion to ASONB is not supported because ASON and JSON do not provide all the schema and type information needed for ASONB; converting from either format to ASONB requires reading the entire input into memory first.
- Comments in ASON and JSONC are discarded.
- ASONB stores DateTime values as UTC timestamps. Converting an ASON DateTime to ASONB and back therefore loses its original time offset; converting an ASONB DateTime to JSON produces a UTC date-time string.
- Converting between JSON and ASON/ASONB may lose type information or precision, so a round-trip conversion does not necessarily reproduce the original document.
- JSON conversion does not preserve the distinction between a Map and a List, the Enumeration type name, or the `Option::Some` wrapper.

## 3 Conversion Rules

### 3.1 Converting between ASON and ASONB

Converting between ASON and ASONB is generally straightforward, as both formats share the same data types and constraints. See [Conversion to and from ASON](https://github.com/hemashushu/asonb#45-conversion-to-and-from-ason) for details.

### 3.2 Converting ASON/ASONB to JSON

JSON has a limited set of data types compared to ASON and ASONB. Specifically, JSON only supports:

- Null
- Boolean
- Number
- String
- Array
- Object

As a result, certain ASON/ASONB types must be mapped to compatible JSON types.

The following table summarizes the conversion rules from ASON/ASONB to JSON:

| ASON/ASONB Type                          | JSON Type                       |
|------------------------------------------|---------------------------------|
| Integer                                  | Number                          |
| Floating-point                           | Number                          |
| Boolean                                  | Boolean                         |
| Character                                | String                          |
| String                                   | String                          |
| DateTime                                 | String in RFC 3339 format       |
| Byte Data                                | String in Base64 format         |
| List                                     | Array                           |
| Tuple                                    | Array                           |
| Object                                   | Object                          |
| Map                                      | Array                           |
| Enumeration                              | Object                          |
| `Option::Some(value)` / `Option::None`   | `value` / `null`                |

#### 3.2.1 Number

- Integers outside the range [-(2^53 - 1), 2^53 - 1] may lose precision when parsed by JSON implementations that use double-precision floating-point numbers, such as JavaScript (see the [ECMAScript specification](https://tc39.es/ecma262/multipage/ecmascript-data-types-and-values.html#sec-ecmascript-language-types-number-type)).
- Special floating-point values, such as `NaN`, `Inf`, and `-Inf`, cannot be represented in JSON and conversion fails.
- JSON numbers do not preserve whether a value was originally `f32` or `f64`.

Byte Data is encoded using the standard Base64 alphabet defined in [RFC 4648](https://datatracker.ietf.org/doc/html/rfc4648#section-4), with `=` padding.

#### 3.2.2 Map

Maps are represented in JSON as arrays of key-value pair objects. Each pair is an object with `key` and `value` fields, and the source order is preserved:

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

For example, the following ASONB map:

```text
[
    "red": "#FF0000",
    "green": "#00FF00"
]
```

would be represented in JSON as:

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

#### 3.2.3 Enumeration

Enumerations are represented in JSON in adjacently tagged form: each value becomes an object containing a `variant` field and, if the variant carries a value, a `value` field:

```text
{
  "variant": VARIANT_NAME,
  "value": VALUE_IF_PRESENT
}
```

Consider the following enumeration:

```rust
enum Action {
    Quit,
    Say(String),
    Take(String, i32),
    Move { x: i32, y: i32 },
}
```

The variants of `Action` are represented in JSON as follows:

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

Note that the enumeration name (`Action` in this example) is not included in the JSON representation.

Variants that carry a single Tuple or Object value are represented as arrays or objects, respectively, just like Tuple-like and Struct-like variants.

#### 3.2.4 `Option`

Options are represented in JSON as either `null` for `None` or the value itself for `Some(value)`. For example:

```text
Option::None -> null
Option::Some(VALUE) -> VALUE
```

### 3.3 Converting from JSON to ASON/ASONB

When converting to ASON/ASONB, the entire JSON structure must be taken into account, since context may affect how certain values, such as `null`, are interpreted.

This table summarizes how JSON types are converted to ASON/ASONB types:

| JSON Type | ASON/ASONB Type                                                                         |
|-----------|-----------------------------------------------------------------------------------------|
| Number    | `i32`, `i64`, or `u64` if in range; otherwise `f64`                                      |
| Boolean   | Boolean                                                                                 |
| String    | String                                                                                  |
| Array     | List if the elements have the same ASON type after promotion, otherwise an error        |
| Object    | Object if all keys are valid identifiers; otherwise, Map if its values are homogeneous |

#### 3.3.1 Numbers

Numbers containing a decimal point (`.`) or an exponent (`e` or `E`) are treated as floating-point numbers and are always converted to `f64`. Other numbers are treated as integers and are converted to various integer types depending on their range:

- `i32`: if its value is within `[i32::MIN, i32::MAX]`
- `i64`: if its value is within `[i64::MIN, i64::MAX]`
- `u64`: if its value is larger than `i64::MAX` and less than or equal to `u64::MAX`
- `f64`: if an integer is outside the ranges of `i32`, `i64`, and `u64`.

#### 3.3.2 Array

Arrays are converted to Lists if all elements have the same ASON type after numeric promotion; otherwise, conversion fails.

For example:

```json
[1, 2, 3]
```

is converted to a List of integers. However, the following array cannot be converted, because its elements are not homogeneous:

```json
[1, "two", true]
```

The type of an array is determined by the ASON types of its elements, not by its length. After recursively converting nested values and applying numeric type promotion, all elements must have the same ASON type. If the type of an empty array cannot be inferred from its context or sibling elements, it defaults to a List of `i32`. For arrays containing objects or `null`, see the following sections.

If an array contains nested arrays, each nested array must be homogeneous, and all nested arrays must have the same ASON type. For example,

```json
[[1, 2], [3, 4]]
```

is converted to a List of Lists of integers. The following arrays cannot be converted: the first contains inner arrays with incompatible element types, and the second contains inner arrays with different ASON types.

```json
[[1, 2], ["one", "two"]]
```

```json
[[1], ["one"]]
```

Empty nested arrays are assigned the type inferred from their sibling arrays when possible. Otherwise, they default to a List of `i32`. For example,

```json
[[], ["foo", "bar"]]
```

is converted to a List of Lists of String, with the empty nested array being treated as a List of `String`.

#### 3.3.3 Object

Objects are converted to ASON/ASONB objects if all keys are valid identifiers.

For example:

```json
{"id": 1, "name": "Alice"}
```

is converted to an ASON/ASONB Object.

If any key is not a valid identifier, the object can be converted to a Map if its values have the same ASON type after numeric promotion.

For example, the following object can be converted to a Map:

```json
{"asonb-derive": "3.0", "proc-macro2": "1.0"}
```

If a JSON object cannot be converted to an ASON/ASONB Object or Map, conversion fails.

JSON objects with duplicate keys are rejected. Strings containing unpaired surrogate escapes are also rejected because ASON strings require valid Unicode scalar values.

For an array of objects, all items are examined to determine the keys and value types, since some keys may be missing from some items. A missing key is treated as having a `null` value. For each key, its non-null values must have the same ASON type after numeric promotion; otherwise, conversion fails. A missing or explicitly null value is converted to `Option::None`, while non-null values for that key are wrapped in `Option::Some(value)`.

If the keys of an empty object cannot be determined from context, the conversion fails, because ASON/ASONB does not allow Objects without keys.

#### 3.3.4 Handling `null` Values

A standalone `null` is converted to `Option::None` of type `Option<i32>`. A `null` object property is likewise converted to `Option::None` of type `Option<i32>`, unless its type can be inferred from context. In an ordinary object, only the property whose value is `null` becomes an Option; other properties are converted independently.

When `null` appears in an array, or as a value for the same key across objects in an array, it is converted to `Option::None`, and the other values at that position or key are wrapped in `Option::Some(value)`. The type of the `Option` is determined by the non-null values; if all values are `null`, it defaults to `Option<i32>`. If the non-null values have different ASON types after numeric promotion, conversion fails.

#### 3.3.5 Numeric Type Promotion

When determining a common ASON type for numeric array elements, Map values, or values of the same key across an array of objects, apply these promotion rules:

- `i32` and `i64` values are promoted to `i64`.
- Signed integer values (`i32` or `i64`) mixed with `u64` values are promoted to `u64` if all values are non-negative; otherwise, conversion fails.
- Integer values mixed with floating-point values are promoted to `f64`.
- Other mixtures that cannot be represented by one ASON type cause conversion to fail.

Promotion to `f64` may lose integer precision for large values.

## 4. Installation

You can install ASON Converter using Cargo:

```sh
cargo install ason-converter
```

## 5. Usage

ASON Converter provides two commands: `ason2b` and `ason2j`.

### `ason2b`

```text
NAME
    ason2b - Convert ASON to and from ASONB

SYNOPSIS
    ason2b [options] [INPUT] [-o OUTPUT]

DESCRIPTION
    The conversion direction is determined automatically from the input and output file names, or from the header of the input data if no file name is provided. In other words, by default the command automatically converts between ASON and ASONB in either direction.

    Use `-t` to indicate conversion from ASONB to ASON, or `-b` to indicate conversion from ASON to ASONB.

    If no input file is specified, input is read from standard input.

    If no output file is specified, output is written to standard output.

OPTIONS
    -t, --text          Indicate conversion from ASONB to ASON
    -b, --binary        Indicate conversion from ASON to ASONB (Default)
    -o OUTPUT           Specify the output file
    --complete SHELL    Generate a shell completion script for the specified shell
                        Supported shells: bash, zsh, fish, powershell
    -h, --help          Show help information
    --version           Show version information

EXIT STATUS

    The command exits with a status code indicating the result of the conversion:
    0   Successful conversion
    1   An error occurred during conversion
    2   No conversion was performed
    3   Input is ASONB while the `-b` / `--binary` flag is specified
    4   Input is ASON while the `-t` / `--text` flag is specified
    5   Writing ASONB to a terminal is an error

EXAMPLES
    * Convert ASON file to ASONB file
        ason2b input.ason -o output.asonb

    * Convert ASONB file to ASON file
        ason2b input.asonb -o output.ason

    * Convert ASON from stdin to ASONB file
        echo '{key: "value"}' | ason2b -o output.asonb

    * Convert ASON from stdin to ASONB and then back to ASON
        echo '{key: "value"}' | ason2b | ason2b
```

### `ason2j`

```text
NAME
    ason2j - Convert ASON/ASONB to and from JSON

SYNOPSIS
    ason2j [options] [INPUT] [-o OUTPUT]

DESCRIPTION
    The conversion direction and encoding are determined automatically from the input and output file names. "*.json" and "*.jsonc" indicate JSON, "*.ason" indicates ASON, and "*.asonb" indicates ASONB.

    If no file name is provided, ASON/ASONB is converted to JSON by default. Use `-r` / `--reverse` to convert JSON to ASON/ASONB. When converting JSON, output is ASON by default; use `-t` / `--text` for ASON or `-b` / `--binary` for ASONB.

    If no input file is specified, input is read from standard input.

    If no output file is specified, output is written to standard output.

OPTIONS
    -r, --reverse       Convert JSON to ASON/ASONB
    -t, --text          Indicate to convert to ASON (Default)
    -b, --binary        Indicate to convert to ASONB
    -o OUTPUT           Specify the output file
    --complete SHELL    Generate a shell completion script for the specified shell
                        Supported shells: bash, zsh, fish, powershell
    -h, --help          Show help information
    --version           Show version information

EXIT STATUS
    The command exits with a status code indicating the result of the conversion:
    0   Successful conversion
    1   An error occurred during conversion
    2   No conversion was performed
    3   Output file name is "*.ason" while the `-b` flag is specified
    4   Output file name is "*.asonb" while the `-t` flag is specified
    5   Writing ASONB to a terminal is an error

EXAMPLES
    * Convert ASON/ASONB file to JSON file
        ason2j input.ason -o output.json
        ason2j input.asonb -o output.json

    * Convert JSON file to ASON/ASONB file
        ason2j input.json -o output.ason
        ason2j input.json -o output.asonb

    * Convert JSON from stdin to ASON on stdout
        echo '{"key": "value"}' | ason2j -r

    * Convert JSON file to ASONB and pipe to another command
        ason2j input.json -b | some_command
```

## 6. Links

- [ASON Specification](https://github.com/hemashushu/ason/blob/main/docs/specification.md)
- [ASONB Specification](https://github.com/hemashushu/asonb/blob/main/README.md)
- [ASON Library (Rust)](https://github.com/hemashushu/ason)
- [ASONB Library (Rust)](https://github.com/hemashushu/asonb)

## 7. License

This project is licensed under the MPL 2.0 License with additional terms. See [LICENSE](./LICENSE) and [LICENSE.additional](./LICENSE.additional).
