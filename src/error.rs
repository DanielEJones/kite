use std::{fmt, io};

use crate::{
    checker::TypeError,
    interpreter::EvalError,
    lexer::Span,
    parser::{Ast, ParseError},
};

#[derive(Debug)]
pub struct Location {
    pub file: String,
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.col)
    }
}

#[derive(Debug)]
pub struct Diagnostic {
    location: Option<Location>,
    message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.location {
            Some(loc) => write!(f, "{}: {}", loc, self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

pub struct SourceMap {
    files: Vec<(String, String)>,
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap { files: Vec::new() }
    }

    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|(_, t)| t.as_str())
    }

    pub fn add(&mut self, file: &str, text: &str) {
        self.files.push((file.to_string(), text.to_string()));
    }

    pub fn locate(&self, span: Span) -> Option<Location> {
        let (path, text) = self.files.get(span.file as usize)?;
        let before = text.get(..span.start)?;

        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let col = before[line_start..].chars().count() + 1;

        Some(Location {
            file: path.clone(),
            line,
            col,
        })
    }

    fn diagnostic(&self, span: Span, message: String) -> Diagnostic {
        Diagnostic {
            location: self.locate(span),
            message,
        }
    }

    pub fn parse_error(&self, e: &ParseError) -> Error {
        Error::Parse(self.diagnostic(e.span, e.message()))
    }

    pub fn type_error(&self, e: &TypeError, ast: &Ast) -> Error {
        Error::Type(self.diagnostic(e.span, e.message(ast)))
    }

    pub fn runtime_error(&self, e: &EvalError) -> Error {
        Error::Runtime(self.diagnostic(e.span, e.message()))
    }
}

pub enum Error {
    IO { path: String, source: io::Error },
    Unbound(String),
    Parse(Diagnostic),
    Type(Diagnostic),
    Runtime(Diagnostic),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::IO { path, source } => write!(f, "{path}: {source}"),
            Error::Unbound(n) => write!(f, "No value named `{n}` in scope."),
            Error::Parse(d) => write!(f, "parse error: {d}"),
            Error::Type(d) => write!(f, "type error: {d}"),
            Error::Runtime(d) => write!(f, "runtime error: {d}"),
        }
    }
}
