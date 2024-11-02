use clap::Parser;
use regex::Regex;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::process;

const ABOUT_TEXT: &str = r#"
Extract the specified columns from FILES or stdin.

Column numbering starts at 1, not 0; column 0 is the entire line, just like awk.
Column numbers that are out of bounds are silently ignored.  When each line is
split, empty leading or trailing columns will be discarded _before_ columns are
extracted.

Negative column numbers are accepted; -1 is the last column, -2 is the second
last, etc.  Note that negative column numbers may not behave as you expect when
files have a variable number of columns per line: e.g. in line 1 column -1 is
column 10, but in line 2 column -1 is column 5.
You need to put -- before the first negative column number, otherwise it will be
interpreted as a non-existent option.

Column ranges of the form 3:8, -3:1, 7:-7, and -1:-3 are accepted.  Both start
and end are required for each range.  It is not an error to specify an end point
that is out of bounds for a line, so 3:1000 will print all columns from 3
onwards (unless you have a *very* long line).
"#;

#[derive(Debug, Parser)]
#[command(version, about, long_about = ABOUT_TEXT)]
struct Flags {
    // Providing a default value makes it optional.
    #[arg(
        short,
        long,
        help = "Regex delimiting input columns; defaults to whitespace",
        default_value = "\\s+"
    )]
    delimiter: Option<String>,
    // Providing a default value makes it optional.
    #[arg(
        short,
        long,
        help = "Separator between output columns; defaults to a single space",
        default_value = " "
    )]
    separator: Option<String>,

    #[arg(
        help = "Leading arguments that look like column specifiers are used as\ncolumn specifiers, then remaining arguments are used as filenames"
    )]
    columns_then_files: Vec<String>,
}

/// Read from all the provided files, reading from the next file when the end of the current file
/// is reached.  Reads from Stdin if a filename is "-".
struct MultipleFileReader {
    filehandles: Vec<Box<dyn Read>>,
}

impl MultipleFileReader {
    /// Initialises and returns a MultipleFileReader from a list of filenames.
    ///
    /// All the filenames provided will be opened eagerly, so problems related to permissions or
    /// existence will be detected by new and the error from [File::open](std::fs::File::open) will
    /// be returned.
    ///
    /// A filename of "-" will use [std::io::stdin] to open stdin.
    ///
    /// If filenames is empty a filename of "-" will be used instead.  This supports the standard
    /// Unix idiom of "if filenames are given read them, else read from stdin".
    fn new(filenames: Vec<String>) -> Result<Self, std::io::Error> {
        Self::new_with_opener(filenames, std::io::stdin)
    }

    // This function allows tests to use dependency injection to check that the correct function is
    // called when the filename is "-".  This is not intended for others to use.
    fn new_with_opener<Closure>(
        filenames: Vec<String>,
        mut stdin_opener: Closure,
    ) -> Result<Self, std::io::Error>
    where
        Closure: FnMut() -> std::io::Stdin,
    {
        if filenames.is_empty() {
            return Self::new_with_opener(vec![String::from("-")], stdin_opener);
        }

        let mut filehandles: Vec<Box<dyn Read>> = Vec::with_capacity(filenames.len());
        for filename in filenames {
            if filename == "-" {
                filehandles.push(Box::new(stdin_opener()));
            } else {
                filehandles.push(Box::new(File::open(filename)?));
            }
        }
        Ok(Self::new_from_filehandles(filehandles))
    }

    /// Initialises and returns a MultipleFileReader from a list of filehandles (anything
    /// implementing the [std::io::Read] trait).  Uses the filehandles unchanged, so they can
    /// point to anything: files, stdin, sockets, ...
    fn new_from_filehandles(filehandles: Vec<Box<dyn Read>>) -> MultipleFileReader {
        Self { filehandles }
    }
}

/// Implements the [std::io::Read] trait for MultipleFileReader.
impl Read for MultipleFileReader {
    /// - A single read() will not return data from two inputs.
    /// - Advances to the next input when a read() from the current input filehandle returns 0, so
    ///   an input filehandle that returns 0 rather than blocking until data is available will not
    ///   be retried.
    /// - The current input filehandle will be discarded when moving on to the next input, so it
    ///   will automatically be closed.
    /// - Errors from underlying read() calls are returned *without* advancing to the next input.
    ///   read() will return errors while the underlying read() call continues to return errors.
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while !self.filehandles.is_empty() {
            let length = self.filehandles[0].read(buf)?;
            if length > 0 {
                return Ok(length);
            }
            // Filehandle has run out of data.
            self.filehandles.remove(0);
        }
        // Run out of files to read.
        Ok(0)
    }
}

// Holds a single column range, start-end inclusive, with start==end for single column ranges.
#[derive(Debug, PartialEq)]
struct ColumnRange {
    start: isize,
    end: isize,
}

// Parse a string that *might* represent a column range.
fn parse_column_range(maybe_column: &str) -> Option<ColumnRange> {
    if let Ok(single_column) = maybe_column.parse::<isize>() {
        return Some(ColumnRange {
            start: single_column,
            end: single_column,
        });
    }

    let regex = Regex::new(r"^(-?\d+):(-?\d+)$").unwrap();
    if let Some(matches) = regex.captures(maybe_column) {
        return Some(ColumnRange {
            start: matches[1].parse::<isize>().unwrap(),
            end: matches[2].parse::<isize>().unwrap(),
        });
    }

    None
}

// Split a list of arguments into leading column ranges and remaining filenames.  Returns parsed
// column ranges and untouched filenames.  This short function is standalone rather than inlined
// into realmain() because it's easier to test in isolation.
fn separate_args(args: Vec<String>) -> (Vec<ColumnRange>, Vec<String>) {
    let columns: Vec<ColumnRange> = args.iter().map_while(|x| parse_column_range(x)).collect();
    let filenames: Vec<String> = args[columns.len()..].to_vec();
    (columns, filenames)
}

// Extract and return the columns specified by column_ranges from the input columns.  Out of bounds
// columns will be silently ignored.  The input columns must live for as long as the returned
// columns, because references are returned rather than copies.  To meet user expectations,
// columns[0] must be the whole input line.
fn extract_columns<'a>(column_ranges: &[ColumnRange], columns: &'a [&'a str]) -> Vec<&'a str> {
    let mut results = vec![];
    for column_range in column_ranges.iter() {
        let indices: Vec<isize> = if column_range.start < column_range.end {
            (column_range.start..=column_range.end).collect()
        } else {
            (column_range.end..=column_range.start).rev().collect()
        };
        for i in indices {
            let j = if i < 0 {
                // Convert separately because it's much easier than combining conversion and math.
                let len: isize = columns.len().try_into().unwrap();
                i + len
            } else {
                i
            };
            if j < 0 {
                continue;
            }
            let k: usize = j.try_into().unwrap();
            if k >= columns.len() {
                continue;
            }
            results.push(columns[k])
        }
    }
    results
}

// A thin wrapper around println!.  This allows me to do dependency injection during tests to
// validate that the correct data would have been output.
fn println_wrapper(print_me: String) {
    println!("{}", print_me);
}

// main(), but testable.  Uses output_handler to print so that tests can do dependency injection to
// validate that the correct data is generated.  I'm using dependency injection rather than
// accumulating a giant array so that processing large files doesn't require memory proportional to
// the file sizes.
fn realmain<T: FnMut(String)>(flags: Flags, mut output_handler: T) -> i32 {
    // TODO: handle the failure case so I can display a nicer error message.
    let delimiter = Regex::new(
        flags
            .delimiter
            .expect("Internal error: flags should have a default value for delimiter")
            .as_str(),
    )
    .unwrap();

    let separator = flags
        .separator
        .expect("Internal error: flags should have a default value for separator");

    let (column_ranges, filenames) = separate_args(flags.columns_then_files);
    if column_ranges.is_empty() {
        eprintln!("At least one column or column range must be provided.");
        return 1;
    }
    let input = MultipleFileReader::new(filenames).unwrap();

    for line in BufReader::new(input).lines() {
        let line = line.unwrap();
        let mut all_columns: Vec<&str> = delimiter
            .split(&line)
            .filter(|col| !col.is_empty())
            .collect();
        all_columns.insert(0, &line);
        let wanted_columns = extract_columns(&column_ranges, &all_columns);
        output_handler(wanted_columns.join(&separator));
    }
    0
}

fn main() {
    process::exit(realmain(Flags::parse(), println_wrapper));
}

#[cfg(test)]
mod clap_test {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn verify() {
        Flags::command().debug_assert();
    }

    #[test]
    fn parse_args() {
        // Checks that I've configured the parser correctly.
        let flags = Flags::parse_from(vec!["argv0", "1"]);
        assert_eq!(Some("\\s+"), flags.delimiter.as_deref());

        let flags = Flags::parse_from(vec![
            "argv0",
            "--separator",
            "asdf",
            "--delimiter",
            "qwerty",
            "1",
        ]);
        assert_eq!(Some("asdf"), flags.separator.as_deref());
        assert_eq!(Some("qwerty"), flags.delimiter.as_deref());
    }
}

#[cfg(test)]
mod multiple_file_reader {
    use super::*;
    use std::io::BufRead;
    use std::io::BufReader;

    /// An implementation of [std::io::Read] that always fails with [std::io::Error] derived from
    /// std::io::ErrorKind::Other.
    struct ReadAlwaysFails {}

    impl Read for ReadAlwaysFails {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(std::io::ErrorKind::Other, "oh no!"))
        }
    }

    #[test]
    fn stdin() {
        let mut call_count = 0;
        let wrapper = || {
            call_count += 1;
            std::io::stdin()
        };
        MultipleFileReader::new_with_opener(vec![String::from("-")], wrapper).unwrap();
        assert_eq!(1, call_count);
    }

    #[test]
    fn empty_vec_means_stdin() {
        let mut call_count = 0;
        let wrapper = || {
            call_count += 1;
            std::io::stdin()
        };
        MultipleFileReader::new_with_opener(vec![], wrapper).unwrap();
        assert_eq!(1, call_count);
    }

    #[test]
    fn stdin_and_files() {
        let mut call_count = 0;
        let wrapper = || {
            call_count += 1;
            std::io::stdin()
        };

        // Opening twice isn't very useful, but should work.
        MultipleFileReader::new_with_opener(
            vec![
                String::from("-"),
                String::from("testdata/file1"),
                String::from("-"),
                String::from("testdata/file2"),
            ],
            wrapper,
        )
        .unwrap();
        assert_eq!(2, call_count);
    }

    #[test]
    fn one_file() {
        let multi_file_reader =
            MultipleFileReader::new(vec![String::from("testdata/file1")]).unwrap();
        let lines: Vec<String> = BufReader::new(multi_file_reader)
            .lines()
            .map(|l| l.unwrap())
            .collect();
        let expected = vec![
            String::from("This is file 1."),
            String::from(""),
            String::from("It is not very interesting."),
        ];
        assert_eq!(expected, lines);
    }

    #[test]
    fn multiple_files() {
        let filenames = vec![
            String::from("testdata/file1"),
            String::from("testdata/file2"),
            String::from("testdata/file3"),
        ];
        let multi_file_reader = MultipleFileReader::new(filenames).unwrap();
        let lines: Vec<String> = BufReader::new(multi_file_reader)
            .lines()
            .map(|l| l.unwrap())
            .collect();
        let expected = vec![
            String::from("This is file 1."),
            String::from(""),
            String::from("It is not very interesting."),
            String::from("File 2 isn't really any better than file 1."),
            String::from(""),
            String::from(""),
            String::from("It has more blank lines.  Including a trailing blank line."),
            String::from(""),
            String::from(
                "File 3 is just here to tell you that the next file was Lorem Ipsum but I",
            ),
            String::from("deleted it."),
        ];
        assert_eq!(expected, lines);
    }

    #[test]
    fn open_fails() {
        let filenames = vec![
            String::from("testdata/file1"),
            String::from("testdata/file_does_not_exist"),
            String::from("testdata/file3"),
        ];
        let multi_file_reader = MultipleFileReader::new(filenames);
        assert!(multi_file_reader.is_err());
    }

    #[test]
    fn read_fails() {
        // We construct a filehandle that errors followed by a valid filehandle.
        // Reads should consistently fail rather than moving on to the valid filehandle.
        let filehandles: Vec<Box<dyn Read>> = vec![
            Box::new(ReadAlwaysFails {}),
            Box::new(File::open("testdata/file1").expect("open(testdata/file1) failed?")),
        ];
        let mut multi_file_reader = MultipleFileReader::new_from_filehandles(filehandles);
        let mut buffer = [0; 10];
        assert!(multi_file_reader.read(&mut buffer).is_err());
        assert!(multi_file_reader.read(&mut buffer).is_err());
        assert!(multi_file_reader.read(&mut buffer).is_err());
    }
}

#[cfg(test)]
mod println_wrapper {
    use super::*;

    #[test]
    fn simple_test() {
        println_wrapper(String::from("printed by println_wrapper test."));
    }
}

#[cfg(test)]
mod realmain {
    use super::*;

    #[test]
    fn expected_columns() {
        let expected = vec![String::from("This"), String::from(""), String::from("It")];
        let mut output_strings: Vec<String> = vec![];
        let output_handler = |output_string: String| {
            output_strings.push(output_string);
        };
        let status = realmain(
            Flags::parse_from(vec!["argv0", "1", "testdata/file1"]),
            output_handler,
        );
        assert_eq!(0, status);
        assert_eq!(expected, output_strings);
    }

    #[test]
    fn empty_columns() {
        let expected = vec![String::from("empty after")];
        let mut output_strings: Vec<String> = vec![];
        let output_handler = |output_string: String| {
            output_strings.push(output_string);
        };
        let status = realmain(
            Flags::parse_from(vec![
                "argv0",
                "1",
                "--",
                "-1",
                "testdata/file_with_empty_columns",
            ]),
            output_handler,
        );
        assert_eq!(0, status);
        assert_eq!(expected, output_strings);
    }

    #[test]
    fn change_delimiter() {
        let expected = vec![String::from("  empty  column  ")];
        let mut output_strings: Vec<String> = vec![];
        let output_handler = |output_string: String| {
            output_strings.push(output_string);
        };
        let status = realmain(
            Flags::parse_from(vec![
                "argv0",
                "--delimiter",
                "before",
                "1",
                "testdata/file_with_empty_columns",
            ]),
            output_handler,
        );
        assert_eq!(0, status);
        assert_eq!(expected, output_strings);
    }

    #[test]
    fn change_separator() {
        let expected = vec![String::from("emptyASDFafter")];
        let mut output_strings: Vec<String> = vec![];
        let output_handler = |output_string: String| {
            output_strings.push(output_string);
        };
        let status = realmain(
            Flags::parse_from(vec![
                "argv0",
                "--separator",
                "ASDF",
                "1",
                "--",
                "-1",
                "testdata/file_with_empty_columns",
            ]),
            output_handler,
        );
        assert_eq!(0, status);
        assert_eq!(expected, output_strings);
    }

    fn panic_if_called(message: String) {
        panic!("output_handler should not have been called!  {message}");
    }

    #[test]
    #[should_panic(expected = "output_handler should not have been called")]
    fn panic_if_called_works() {
        panic_if_called(String::from("this should panic"));
    }

    #[test]
    fn no_columns() {
        let status = realmain(
            Flags::parse_from(vec!["argv0", "testdata/file1"]),
            panic_if_called,
        );
        assert_eq!(1, status);
    }
}

#[cfg(test)]
mod parse_column_range {
    use super::*;

    #[test]
    fn parse_single_column() {
        assert_eq!(
            Some(ColumnRange { start: 1, end: 1 }),
            parse_column_range("1")
        );
        assert_eq!(
            Some(ColumnRange { start: -2, end: -2 }),
            parse_column_range("-2")
        );
    }

    #[test]
    fn parse_multiple_columns() {
        assert_eq!(
            Some(ColumnRange { start: 1, end: 7 }),
            parse_column_range("1:7")
        );
        assert_eq!(
            Some(ColumnRange { start: -6, end: -2 }),
            parse_column_range("-6:-2")
        );
        assert_eq!(
            Some(ColumnRange { start: 3, end: -2 }),
            parse_column_range("3:-2")
        );
    }

    #[test]
    fn rejected() {
        assert_eq!(None, parse_column_range("a"));
        assert_eq!(None, parse_column_range("1.2"));
        assert_eq!(None, parse_column_range("1:a"));
        assert_eq!(None, parse_column_range("1:2-"));
        assert_eq!(None, parse_column_range(":2"));
        assert_eq!(None, parse_column_range("1:"));
    }
}

#[cfg(test)]
mod separate_args {
    use super::*;

    #[test]
    fn no_args() {
        let (columns, filenames) = separate_args(vec![]);
        assert_eq!(Vec::<ColumnRange>::new(), columns);
        assert_eq!(Vec::<String>::new(), filenames);
    }

    #[test]
    fn columns_then_files() {
        let (actual_columns, actual_filenames) = separate_args(vec![
            String::from("1"),
            String::from("4:-2"),
            String::from("foo"),
            String::from("bar"),
            String::from("baz"),
        ]);
        let expected_columns = vec![
            ColumnRange { start: 1, end: 1 },
            ColumnRange { start: 4, end: -2 },
        ];
        assert_eq!(expected_columns, actual_columns);
        let expected_filenames = vec![
            String::from("foo"),
            String::from("bar"),
            String::from("baz"),
        ];
        assert_eq!(expected_filenames, actual_filenames);
    }

    #[test]
    fn mixed_columns_and_files() {
        let (actual_columns, actual_filenames) = separate_args(vec![
            String::from("4:-2"),
            String::from("foo"),
            String::from("bar"),
            String::from("1"),
            String::from("baz"),
        ]);
        let expected_columns = vec![ColumnRange { start: 4, end: -2 }];
        assert_eq!(expected_columns, actual_columns);
        let expected_filenames = vec![
            String::from("foo"),
            String::from("bar"),
            String::from("1"),
            String::from("baz"),
        ];
        assert_eq!(expected_filenames, actual_filenames);
    }
}

#[cfg(test)]
mod extract_columns {
    use super::*;

    #[test]
    fn single_column() {
        let expected = vec!["asdf"];
        let column_ranges = [ColumnRange { start: 1, end: 1 }];
        let columns = ["ignored", "asdf", "ignored"];
        let actual = extract_columns(&column_ranges, &columns);
        assert_eq!(expected, actual);
    }

    #[test]
    fn out_of_bounds_column() {
        let expected: Vec<&str> = vec![];
        let column_ranges = [ColumnRange { start: 7, end: 7 }];
        let columns = ["ignored", "ignored", "ignored"];
        let actual = extract_columns(&column_ranges, &columns);
        assert_eq!(expected, actual);
    }

    #[test]
    fn single_range() {
        let expected = vec!["two", "three", "four"];
        let column_ranges = [ColumnRange { start: 2, end: 4 }];
        let columns = ["zero", "one", "two", "three", "four", "five"];
        let actual = extract_columns(&column_ranges, &columns);
        assert_eq!(expected, actual);
    }

    #[test]
    fn partially_out_of_bounds_range() {
        let expected = vec!["two", "three", "four", "five"];
        let column_ranges = [ColumnRange { start: 2, end: 6 }];
        let columns = ["zero", "one", "two", "three", "four", "five"];
        let actual = extract_columns(&column_ranges, &columns);
        assert_eq!(expected, actual);
    }

    #[test]
    fn negative_index() {
        let expected = vec!["four", "five", "zero", "one", "two", "three"];
        let column_ranges = [ColumnRange { start: -2, end: 3 }];
        let columns = ["zero", "one", "two", "three", "four", "five"];
        let actual = extract_columns(&column_ranges, &columns);
        assert_eq!(expected, actual);
    }

    #[test]
    fn overlapping_ranges() {
        let expected = vec![
            "two", "three", "four", "one", "two", "three", "four", "five",
        ];
        let column_ranges = [
            ColumnRange { start: 2, end: 4 },
            ColumnRange { start: 1, end: 5 },
        ];
        let columns = ["zero", "one", "two", "three", "four", "five"];
        let actual = extract_columns(&column_ranges, &columns);
        assert_eq!(expected, actual);
    }

    #[test]
    fn really_out_of_bounds_indices() {
        let column_ranges = [ColumnRange {
            start: -20,
            end: 20,
        }];
        let columns = ["zero", "one", "two", "three", "four", "five"];
        // We'll get all the columns once using -6:-1 and a second time using 0:5.
        let expected = vec![
            "zero", "one", "two", "three", "four", "five", "zero", "one", "two", "three", "four",
            "five",
        ];
        let actual = extract_columns(&column_ranges, &columns);
        assert_eq!(expected, actual);
    }

    #[test]
    fn reversed_indices() {
        let expected = vec!["four", "three", "two"];
        let column_ranges = [ColumnRange { start: 4, end: 2 }];
        let columns = ["zero", "one", "two", "three", "four", "five"];
        let actual = extract_columns(&column_ranges, &columns);
        assert_eq!(expected, actual);
    }
}
