#![allow(dead_code)]
use std::fs::File;
use std::io::Read;

/// Read from all the provided files, reading from the next file when the end of the current file
/// is reached.
struct MultipleFileReader {
    filehandles: Vec<Box<dyn Read>>,
}

impl MultipleFileReader {
    /// Initialises and returns a MultipleFileReader from a list of filenames.
    ///
    /// All the filenames provided will be opened eagerly, so problems related to permissions or
    /// existence will be detected by new and the error from [File::open](std::fs::File::open)
    /// will be returned.
    fn new(filenames: Vec<String>) -> Result<Self, std::io::Error> {
        let mut filehandles: Vec<Box<dyn Read>> = Vec::with_capacity(filenames.len());
        for filename in filenames {
            filehandles.push(Box::new(File::open(filename)?));
        }
        Ok(Self::new_from_filehandles(filehandles))
    }

    /// Initialises and returns a MultipleFileReader from a list of filehandles (anything
    /// implementing the [std::io::Read] trait.  Uses the filehandles unchanged, so they can
    /// point to anything: files, stdin, sockets, ...
    fn new_from_filehandles(filehandles: Vec<Box<dyn Read>>) -> MultipleFileReader {
        Self { filehandles }
    }
}

/// Implements the [std::io::Read] Trait.
impl Read for MultipleFileReader {
    /// - A single read() will not return data from two inputs.
    /// - Advances to the next input when a read() from the current input filehandle returns 0, so
    ///   an input filehandle that returns 0 rather than blocking until data is available will not
    ///   be retried.
    /// - The current input filehandle will be discarded when moving on to the next input, so it
    ///   will automatically be closed.
    /// - Errors from underlying read() calls are returned *without* advancing to the next input
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

/// Returns a MultipleFileReader (if filenames are provided) or stdin if no filenames are provided.
struct StdinOrFiles {
    reader: Box<dyn Read>,
}

impl StdinOrFiles {
    /// Initialises and returns a StdinOrFiles.
    ///
    /// If filenames are provided, creates and wraps a MultipleFileReader to read all the files.
    /// Otherwise creates and wraps an std:io::stdin().
    fn new(filenames: Vec<String>) -> Result<Self, std::io::Error> {
        if filenames.is_empty() {
            Ok(Self {
                reader: Box::new(std::io::stdin()),
            })
        } else {
            Ok(Self {
                reader: Box::new(MultipleFileReader::new(filenames)?),
            })
        }
    }
}

/// Implements [std::io::Read] for StdinOrFiles.
impl Read for StdinOrFiles {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buf)
    }
}

fn main() {
    println!("Hello, world!");
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
mod stdin_or_files {
    use super::*;
    use std::io::BufRead;
    use std::io::BufReader;

    #[test]
    fn no_files() {
        assert!(StdinOrFiles::new(vec![]).is_ok());
        // TODO: what can I test here?
    }

    #[test]
    fn two_files() {
        let fh = StdinOrFiles::new(vec![
            String::from("testdata/file1"),
            String::from("testdata/file2"),
        ])
        .unwrap();
        let lines: Vec<String> = BufReader::new(fh).lines().map(|l| l.unwrap()).collect();
        let expected = vec![
            String::from("This is file 1."),
            String::from(""),
            String::from("It is not very interesting."),
            String::from("File 2 isn't really any better than file 1."),
            String::from(""),
            String::from(""),
            String::from("It has more blank lines.  Including a trailing blank line."),
            String::from(""),
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
        assert!(StdinOrFiles::new(filenames).is_err());
    }
}
