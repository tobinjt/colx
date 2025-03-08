# colx

Extract the specified columns from FILES or stdin.

Column numbering starts at 1, not 0; column 0 is the entire line, just like awk.
Column numbers that are out of bounds are silently ignored. When each line is
split, empty leading or trailing columns will be discarded _before_ columns are
extracted.

Negative column numbers are accepted; -1 is the last column, -2 is the second
last, etc. Note that negative column numbers may not behave as you expect when
files have a variable number of columns per line: e.g. in line 1 column -1 is
column 10, but in line 2 column -1 is column 5. You need to put -- before the
first negative column number, otherwise it will be interpreted as a non-existent
option.

Column ranges of the form 3:8, -3:1, 7:-7, and -1:-3 are accepted. Both start
and end are required for each range. It is not an error to specify an end point
that is out of bounds for a line, so 3:1000 will print all columns from 3
onwards (unless you have a _very_ long line).

## Usage

```text
Usage: colx [OPTIONS] [COLUMNS_THEN_FILES]...

Arguments:
  [COLUMNS_THEN_FILES]...
          Leading arguments that look like column specifiers are used as
          column specifiers, then remaining arguments are used as filenames

Options:
  -d, --delimiter <DELIMITER>
          Regex delimiting input columns; defaults to whitespace

  -s, --separator <SEPARATOR>
          Separator between output columns; defaults to a single space
```

### Example

```shell
$ grep ^root: /etc/passwd | colx -d : -s '!!!' 1 5
root!!!System Administrator
```

## Installation

`colx` is written in Rust and so needs a Rust toolchain for installation. See
<https://www.rust-lang.org/tools/install> for how to install Rust. When Rust is
installed `colx` can be installed from crates.io with:

```shell
cargo install colx
```

There are no pre-built binaries available, contributions to provide binaries are
welcome.

## License

Licensed under the Apache 2.0 licence, see the [`LICENSE`](LICENSE) file
accompanying the software.
