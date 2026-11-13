# ASON Converter

ASON Converter is a Rust library and a set of command-line tools (`ason2b`, `ason2j`) for converting between ASON, ASONB (ASON's binary format), JSON, and JSONC.

<!-- @import "[TOC]" {cmd="toc" depthFrom=2 depthTo=4 orderedList=false} -->

<!-- code_chunk_output -->

- [1 Features](#1-features)
- [2 Limitations](#2-limitations)
- [3 Conversion Rules](#3-conversion-rules)
  - [3.1 Converting between ASON and ASONB](#31-converting-between-ason-and-asonb)
  - [3.2 Converting from ASON/ASONB to JSON](#32-converting-from-asonasonb-to-json)
    - [3.2.1 Numbers](#321-numbers)
    - [3.2.2 Maps](#322-maps)
    - [3.2.3 Enumerations](#323-enumerations)
    - [3.2.4 `Option`](#324-option)
    - [3.2.5 `Result`](#325-result)
  - [3.3 Converting from JSON to ASON/ASONB](#33-converting-from-json-to-asonasonb)
    - [3.3.1 Numbers](#331-numbers)
    - [3.3.2 Arrays](#332-arrays)
    - [3.3.3 Objects](#333-objects)
    - [3.3.4 Handling `null` Values](#334-handling-null-values)
    - [3.3.5 Numeric Type Promotion](#335-numeric-type-promotion)
    - [3.3.6 Promoting Objects to Maps](#336-promoting-objects-to-maps)
- [4 Command-Line Tools](#4-command-line-tools)
  - [4.1 Installation](#41-installation)
  - [4.2 Commands](#42-commands)
    - [4.2.1 `ason2b`](#421-ason2b)
    - [4.2.2 `ason2j`](#422-ason2j)
- [5 Library Usage](#5-library-usage)
  - [5.1 Publishing the Workspace](#51-publishing-the-workspace)
- [6 Links](#6-links)
- [7 License](#7-license)

<!-- /code_chunk_output -->

## 1 Features

- Converts between ASON, ASONB, and JSON.
- Reads [JSONC](https://jsonc.org/) (JSON with comments).
- Streams ASONB-to-ASON or ASONB-to-JSON conversions when the input meets the requirements below.
- Produces formatted ASON and JSON output.

## 2 Limitations

- ASONB-to-ASON and ASONB-to-JSON conversions can stream only when the root value is a multi-chunk List, or a `Result::Ok` whose payload is a multi-chunk List (as produced by `result_stream_writer`). Conversion to ASONB cannot stream: ASON and JSON do not contain all the schema and type information ASONB requires, so the entire input must be read into memory first.
- Comments in ASON and JSONC input are discarded.
- ASONB stores DateTime values as UTC timestamps. Converting an ASON DateTime to ASONB and back loses the original UTC offset. Converting an ASONB DateTime to JSON produces a UTC date-time string.
- Converting between JSON and ASON/ASONB may lose type information or numeric precision. A round trip may not reproduce the original document exactly.
- JSON cannot preserve the distinction between Map and List, an Enumeration's type name, or the `Some` wrapper of an `Option`.

## 3 Conversion Rules

### 3.1 Converting between ASON and ASONB

ASON and ASONB share the same data types and constraints, so conversion between them is generally straightforward. For details, see [Conversion to and from ASON](https://github.com/hemashushu/asonb#45-conversion-to-and-from-ason).

The predefined enumerations `Option` and `Result` are supported in both their qualified (`Option::Some(1)`, `Result::Err("failed")`) and shorthand (`Some(1)`, `None`, `Ok(1)`, `Err("failed")`) forms. ASON output always uses the shorthand form.

Because ASON text does not declare types, ASON-to-ASONB conversion infers the ASONB schema from the values. In a collection, all elements must have the same type, with these exceptions:

- Enumeration values of the same type are merged, so `[Color::Red, Color::Green]` produces an enumeration type with both variants. A variant must have the same payload type everywhere it appears.
- An `Option` or `Result` branch that never appears takes its type from sibling values, for example `[None, Some("a")]` is a List of `Option<String>` and `[Ok(1), Err("failed")]` is a List of `Result<i32, String>`. If a branch's type cannot be inferred, it defaults to `i32`.

### 3.2 Converting from ASON/ASONB to JSON

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

#### 3.2.1 Numbers

- Integers outside the range [-(2^53 - 1), 2^53 - 1] may lose precision when parsed by JSON implementations that use double-precision floating-point numbers, such as JavaScript (see the [ECMAScript specification](https://tc39.es/ecma262/multipage/ecmascript-data-types-and-values.html#sec-ecmascript-language-types-number-type)).
- Special floating-point values, such as `NaN`, `Inf`, and `-Inf`, cannot be represented in JSON and conversion fails.
- JSON numbers do not preserve whether a value was originally `f32` or `f64`.

Byte data is encoded with the standard Base64 alphabet and `=` padding defined in [RFC 4648, Section 4](https://datatracker.ietf.org/doc/html/rfc4648#section-4).

#### 3.2.2 Maps

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

For example, this ASONB map:

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

#### 3.2.3 Enumerations

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

#### 3.2.4 `Option`

Options map to `null` for `None` and to the contained value for `Some(value)`:

```text
None -> null
Some(VALUE) -> VALUE
```

#### 3.2.5 `Result`

Results are converted like other enumerations:

```text
Ok(VALUE) -> {"variant": "Ok", "value": VALUE}
Err(ERROR) -> {"variant": "Err", "value": ERROR}
```

### 3.3 Converting from JSON to ASON/ASONB

JSON-to-ASON/ASONB conversion considers the entire document because context affects how values such as `null` are interpreted.

The table below summarizes how JSON types map to ASON/ASONB:

| JSON Type | ASON/ASONB Type                                                                         |
|-----------|-----------------------------------------------------------------------------------------|
| Number    | `i32`, `i64`, or `u64` if in range; otherwise `f64`                                      |
| Boolean   | Boolean                                                                                 |
| String    | String                                                                                  |
| Array     | List if the elements have the same ASON type after promotion, otherwise an error        |
| Object    | Object if all keys are valid identifiers; otherwise, Map if its values are homogeneous |

#### 3.3.1 Numbers

Numbers with a decimal point (`.`) or exponent (`e` or `E`) are treated as floating-point values and converted to `f64`. Other numbers are treated as integers and assigned a type based on their range:

- `i32`: if its value is within `[i32::MIN, i32::MAX]`
- `i64`: if its value is within `[i64::MIN, i64::MAX]`
- `u64`: if its value is larger than `i64::MAX` and less than or equal to `u64::MAX`
- `f64`: if an integer is outside the ranges of `i32`, `i64`, and `u64`.

#### 3.3.2 Arrays

An array becomes a List if all its elements have the same ASON type after numeric promotion. Otherwise, conversion fails.

For example:

```json
[1, 2, 3]
```

becomes a List of integers. This array cannot be converted because its elements have different types:

```json
[1, "two", true]
```

An array's type depends on its elements, not its length. After nested values are converted and numeric promotion is applied, all elements must have the same ASON type. An empty array defaults to a List of `i32` when its type cannot be inferred from context or sibling elements. See the following sections for arrays containing objects or `null`.

For nested arrays, each inner array must be homogeneous, and all inner arrays must have the same ASON type. For example:

```json
[[1, 2], [3, 4]]
```

becomes a List of Lists of integers. These arrays cannot be converted: the first has an inner array with mixed element types, and the second has inner arrays with different ASON types.

```json
[[1, 2], ["one", "two"]]
```

```json
[[1], ["one"]]
```

When possible, an empty nested array takes its type from sibling arrays. Otherwise, it defaults to a List of `i32`. For example:

```json
[[], ["foo", "bar"]]
```

becomes a List of Lists of String; the empty inner array is inferred as a List of `String`.

#### 3.3.3 Objects

When converted on its own, an object becomes an ASON/ASONB Object if all its keys are valid identifiers.

For example:

```json
{"id": 1, "name": "Alice"}
```

becomes an ASON/ASONB Object.

The ASON keywords `true`, `false`, `NaN`, `NaN_f32`, `NaN_f64`, `Inf`, `Inf_f32`, and `Inf_f64` are reserved and are not treated as valid keys for an Object. The variant shorthands `Some`, `None`, `Ok`, and `Err` are ordinary identifiers and can be Object keys.

If any key is not a valid identifier, the object becomes a Map only if all its values have the same ASON type after numeric promotion. Objects in a collection may be promoted to Maps to match other elements; see [Promoting Objects to Maps](#336-promoting-objects-to-maps).

For example, the following object can be converted to a Map:

```json
{"asonb-derive": "3.0", "proc-macro2": "1.0"}
```

If a JSON object cannot be converted to an ASON/ASONB Object or Map, conversion fails.

JSON objects with duplicate keys are rejected. Strings containing unpaired surrogate escapes are also rejected because ASON strings require valid Unicode scalar values.

For an array of objects, conversion examines every item to collect the keys and infer value types. A missing key is treated as `null`. For each key, all non-null values must have the same ASON type after numeric promotion; otherwise, conversion fails. Missing and explicitly null values become `Option::None`; non-null values become `Option::Some(value)`.

If context does not provide keys for an empty object, conversion fails because ASON/ASONB Objects must have keys.

#### 3.3.4 Handling `null` Values

A standalone `null` becomes `Option::None` with type `Option<i32>`. A `null` object property uses the same default unless its type can be inferred from context. In an ordinary object, only null-valued properties become Options; other properties are converted independently.

When `null` appears in an array or for a key shared across an array of objects, it becomes `Option::None`; the corresponding non-null values become `Option::Some(value)`. The non-null values determine the Option's type. If all values are null, the type defaults to `Option<i32>`. Conversion fails if the non-null values have incompatible ASON types after numeric promotion.

#### 3.3.5 Numeric Type Promotion

When finding a common ASON type for numeric array elements, Map values, or values for the same key across an array of objects, use these promotion rules:

- `i32` and `i64` values are promoted to `i64`.
- Signed integer values (`i32` or `i64`) mixed with `u64` values are promoted to `u64` if all values are non-negative; otherwise, conversion fails.
- Integer values mixed with floating-point values are promoted to `f64`.
- Other mixtures that cannot be represented by one ASON type cause conversion to fail.

Promotion to `f64` may lose integer precision for large values.

#### 3.3.6 Promoting Objects to Maps

When a collection needs a common ASON type and contains both Objects and Maps, an Object is promoted to a Map if the values can share one ASON type. Each Object field name becomes a String key, and all Object field values and Map values are unified using the numeric promotion rules above. If the values cannot be unified, conversion fails. This promotion occurs only when a Map is already present; Objects are not converted to Maps solely because they appear in a collection.

For example, the first value below is initially an Object and the second is a Map because its key is not a valid identifier:

```json
[{"id": 1}, {"invalid-key": 2147483648}]
```

The Object is promoted to a Map, and `1` is promoted to `i64`. The result is a List of `Map<String, i64>` values.

## 4 Command-Line Tools

Two command-line tools are provided, each in its own package: `ason2b` converts between ASON and ASONB, and `ason2j` converts between ASON/ASONB and JSON.

### 4.1 Installation

From the workspace root, install the CLI tools with Cargo:

```sh
cargo install --path crates/ason2b
cargo install --path crates/ason2j
```

### 4.2 Commands

#### 4.2.1 `ason2b`

```text
NAME
    ason2b - Convert ASON to and from ASONB

USAGE
    ason2b [options] [INPUT]

ARGUMENTS
    INPUT                   Input file (defaults to standard input)

OPTIONS
    -t, --text              Force conversion from ASONB to ASON
    -b, --binary            Force conversion from ASON to ASONB
    -o, --output OUTPUT     Write output to this file (defaults to standard output)
        --complete SHELL    Print a completion script to standard output
                            Supported shells: bash, zsh, fish, PowerShell
    -h, --help              Show help information
    -V, --version           Show version information

EXIT STATUS
    0   Successful conversion
    1   An error occurred during conversion
    2   No conversion was performed
    3   Input is ASONB while `-b` / `--binary` is specified
    4   Input is ASON while `-t` / `--text` is specified
    5   ASONB output is directed to a terminal

DESCRIPTION
    The command determines the conversion direction from the input and output file names. If a file name does not identify the input format, it checks the input header. By default, it converts between ASON and ASONB in either direction.

    Use `-t` to force conversion from ASONB to ASON, or `-b` to force conversion from ASON to ASONB.

    If no input file is specified, input is read from standard input.

    If no output file is specified, output is written to standard output.

EXAMPLES
    * Convert an ASON file to ASONB
        ason2b input.ason -o output.asonb

    * Convert an ASONB file to ASON
        ason2b input.asonb -o output.ason

    * Convert ASON from standard input to an ASONB file
        echo '{key: "value"}' | ason2b -o output.asonb

    * Convert ASON from standard input to ASONB, then back to ASON
        echo '{key: "value"}' | ason2b | ason2b
```

#### 4.2.2 `ason2j`

```text
NAME
    ason2j - Convert ASON/ASONB to and from JSON

USAGE
    ason2j [options] [INPUT]

ARGUMENTS
    INPUT                   Input file (defaults to standard input)

OPTIONS
    -r, --reverse           Treat input without a recognized extension as JSON
    -t, --text              Select ASON output (the default for JSON input)
    -b, --binary            Select ASONB output
    -o, --output OUTPUT     Write output to this file (defaults to standard output)
        --complete SHELL    Print a completion script to standard output
                            Supported shells: bash, zsh, fish, PowerShell
    -h, --help              Show help information
    -V, --version           Show version information

EXIT STATUS
    0   Successful conversion
    1   An error occurred during conversion
    2   No conversion was performed
    3   The output file has the `.ason` extension while `-b` / `--binary` is specified
    4   The output file has the `.asonb` extension while `-t` / `--text` is specified
    5   ASONB output is directed to a terminal

DESCRIPTION
    The command determines the input and output formats from the file names. The extensions `.json` and `.jsonc` indicate JSON, `.ason` indicates ASON, and `.asonb` indicates ASONB. If the input has no recognized extension, the command checks its header for ASONB; otherwise, it treats the input as ASON. Use `-r` to treat input without a recognized extension as JSON.

    By default, ASON or ASONB input is converted to JSON. Use `-r` / `--reverse` to convert JSON input to ASON/ASONB. ASON is the default output format; use `-t` / `--text` to select ASON or `-b` / `--binary` to select ASONB.

    If no input file is specified, input is read from standard input.

    If no output file is specified, output is written to standard output.

EXAMPLES
    * Convert an ASON or ASONB file to JSON
        ason2j input.ason -o output.json
        ason2j input.asonb -o output.json

    * Convert a JSON file to ASON or ASONB
        ason2j input.json -o output.ason
        ason2j input.json -o output.asonb

    * Convert JSON from standard input to ASON on standard output
        echo '{"key": "value"}' | ason2j -r

    * Convert a JSON file to ASONB and pipe it to another command
        ason2j input.json -b | some_command
```

## 5 Library Usage

The core library is provided by the `ason-convert` package. It is not yet published to crates.io, so to use a local checkout, add its path to your project's `Cargo.toml`:

```toml
[dependencies]
ason-convert = { path = "../ason-converter/crates/ason-convert" }
```

The conversion functions return `Result<_, ConversionError>`. `ConversionError` implements `std::error::Error`, so it can be propagated with `?`. For example, convert ASON to ASONB, back to ASON, and then to JSON:

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

For JSON or JSONC input, use `json_to_ason` or `json_to_asonb`. `format_json` parses JSONC, removes comments, and returns formatted JSON. `asonb_to_json` converts a complete ASONB document. The reader functions `asonb_reader_to_ason` and `asonb_reader_to_json` accept `Read`/`Write` streams and stream only when the root value is a multi-chunk List or a `Result::Ok` containing a multi-chunk List.

The `ason-convert` library is separate from the `ason2b` and `ason2j` packages, so depending on it does not add the CLI's `clap` dependencies. Its ASON and ASONB dependencies are resolved from crates.io.

### 5.1 Publishing the Workspace

The [Publish to crates.io](.github/workflows/public.yml) workflow is triggered manually from GitHub Actions. It tests and packages the entire workspace with the lockfile, then publishes `ason-convert`, `ason2b`, and `ason2j` in dependency order. Versions already published to crates.io are skipped.

The workflow uses crates.io trusted publishing (OIDC). Configure a trusted publisher for each crate with the repository `hemashushu/ason-converter` and the workflow filename `public.yml` before running it. No long-lived registry token is stored in GitHub secrets.

## 6 Links

- [ASON Specification](https://github.com/hemashushu/ason/blob/main/docs/specification.md)
- [ASONB Specification](https://github.com/hemashushu/asonb/blob/main/README.md)
- [ASON Library (Rust)](https://github.com/hemashushu/ason)
- [ASONB Library (Rust)](https://github.com/hemashushu/asonb)

## 7 License

This project is licensed under the MPL 2.0 License with additional terms. See [LICENSE](./LICENSE) and [LICENSE.additional](./LICENSE.additional).
