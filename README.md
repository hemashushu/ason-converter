# ASON Converter

ASON Converter is a tool for converting between ASON, ASONB (the binary format of ASON), and JSON.

<!-- @import "[TOC]" {cmd="toc" depthFrom=2 depthTo=4 orderedList=false} -->

<!-- code_chunk_output -->

- [Features](#features)
- [Limitations](#limitations)
- [Conversion Rules](#conversion-rules)
  - [Converting between ASON and ASONB](#converting-between-ason-and-asonb)
  - [Converting ASON/ASONB to JSON](#converting-asonasonb-to-json)
  - [Converting from JSON to ASON/ASONB](#converting-from-json-to-asonasonb)
- [Installation](#installation)
- [Usage](#usage)
  - [`ason2b`](#ason2b)
  - [`ason2j`](#ason2j)

<!-- /code_chunk_output -->

## Features

- Converts between ASON, ASONB, and JSON.
- Supports reading [JSONC](https://jsonc.org/) (JSON with comments).
- Supports streaming conversion from ASONB to ASON.
- Produces well-formatted ASON and JSON output.

## Limitations

- Streaming conversion is only available from ASONB to ASON. Other directions are not supported because ASON and JSON lack schema and type information, so converting from ASON or JSON requires reading the entire input into memory to gather the necessary type information first.
- Comments in ASON and JSONC are discarded.
- DateTime values in ASONB lose precision.
- When converted to ASON or JSON, fixed-capacity Strings, Byte Data, and Lists in ASONB produce Strings containing null characters, Byte Data containing `\0` bytes, and Lists containing padding elements, respectively. This behavior is expected, since fixed-capacity types are mainly used in low-level systems that generally do not require conversion.

## Conversion Rules

### Converting between ASON and ASONB

Converting between ASON and ASONB is generally straightforward, as both formats support the same set of data types. However, converting ASONB to ASON is subject to the restrictions described in the [Limitations](#limitations) section.

### Converting ASON/ASONB to JSON

JSON has a limited set of data types compared to ASON and ASONB. Specifically, JSON only supports:

- Null
- Boolean
- Number
- String
- Array
- Object

As a result, certain ASON/ASONB types must be mapped to compatible JSON types.

The following table shows the conversion rules from ASON/ASONB to JSON:

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
| Map                                      | Object                          |
| Enumeration                              | String                          |
| `Option::Some(value)` and `Option::None` | `value` and `null` respectively |

Note that an enumeration variant carrying a value is converted to a JSON string consisting of the variant name followed by the stringified value. For example:

- `Number::Integer(42)` is converted to the string `"Number::Integer(42)"`.
- `Member::User({id: 123, name: "Alice"})` is converted to the string `"Member::User({\"id\": 123, \"name\": \"Alice\"})"`.

### Converting from JSON to ASON/ASONB

When converting to ASON/ASONB, the entire JSON structure must be taken into account, since context may affect how certain values, such as `null`, are interpreted.

| JSON Type | ASON/ASONB Type                                                                     |
|-----------|-------------------------------------------------------------------------------------|
| Number    | `i32` if it is an integer within `[i32::MIN, i32::MAX]`                             |
| Number    | `i64` if it is an integer within `[i64::MIN, i64::MAX]`                             |
| Number    | `f64` if it is a floating-point number                                              |
| Boolean   | Boolean                                                                             |
| String    | String                                                                              |
| Array     | List if the elements are homogeneous, otherwise Tuple                               |
| Object    | Map if the values are homogeneous, otherwise Object                                 |
| Null      | `Option::None` (non-null values become `Option::Some(value)` if the context allows) |

## Installation

You can install ASON Converter using Cargo:

```sh
cargo install ason-converter
```

## Usage

ASON Converter provides two commands: `ason2b` and `ason2j`.

### `ason2b`

```text
NAME
    ason2b - Convert ASON to and from ASONB

SYNOPSIS
    ason2b [options] [INPUT] [-o OUTPUT]

DESCRIPTION
    Converts ASON to ASONB by default. Use the `-r` flag to convert ASONB to ASON. If no input file is specified, input is read from standard input. If no output file is specified, output is written to standard output.

OPTIONS
    -r                  Convert ASONB to ASON
    -o OUTPUT           Specify the output file
    --complete SHELL    Generate a shell completion script for the specified shell
                        Supported shells: bash, zsh, fish, powershell
    -h, --help          Show help information
    --version           Show version information

EXAMPLES
    * Convert ASON file input.ason to ASONB file output.asonb
        ason2b input.ason -o output.asonb

    * Convert ASONB file input.asonb to ASON file output.ason
        ason2b -r input.asonb -o output.ason

    * Convert ASON from stdin to ASONB file output.asonb
        echo '{key: "value"}' | ason2b -o output.asonb

    * Convert ASON from stdin to ASONB on stdout
        echo '{key: "value"}' | ason2b
```

### `ason2j`

```text
NAME
    ason2j - Convert ASON/ASONB to and from JSON

SYNOPSIS
    ason2j [options] [INPUT] [-o OUTPUT]

DESCRIPTION
    Converts ASON/ASONB to JSON by default. Use the `-r` flag to convert JSON to ASON/ASONB. If no input file is specified, input is read from standard input. If no output file is specified, output is written to standard output.

OPTIONS
    -r                  Convert JSON to ASON/ASONB
    -b                  Use ASONB instead of ASON
    -o OUTPUT           Specify the output file
    --complete SHELL    Generate a shell completion script for the specified shell
                        Supported shells: bash, zsh, fish, powershell
    -h, --help          Show help information
    --version           Show version information

EXAMPLES
    * Convert ASON file input.ason to JSON file output.json
        ason2j input.ason -o output.json

    * Convert ASONB file input.asonb to JSON file output.json
        ason2j -b input.asonb -o output.json

    * Convert JSON file input.json to ASON file output.ason
        ason2j -r input.json -o output.ason

    * Convert JSON file input.json to ASONB file output.asonb
        ason2j -r -b input.json -o output.asonb

    * Convert JSON from stdin to ASON on stdout
        echo '{"key": "value"}' | ason2j -r
```


