use std::{fmt, io, rc::Rc};

use crate::{
    checker::{Checker, TypeError},
    interpreter::{EvalError, EvalResult, Interpreter, Value, Variant},
    lexer::{HOST_FILE, Span},
    parser::{Ast, ParseError, Program, parse_file_into},
};

pub struct EngineBuilder {
    host_fns: Vec<HostDecl>,
    source_files: Vec<(String, String)>,
}

impl EngineBuilder {
    pub fn new() -> EngineBuilder {
        EngineBuilder {
            host_fns: Vec::new(),
            source_files: Vec::new(),
        }
    }

    pub fn add_host_fn(
        mut self,
        name: &str,
        signature: &str,
        f: fn(Span, &[Value]) -> EvalResult<Value>,
    ) -> EngineBuilder {
        self.host_fns.push(HostDecl {
            name: name.to_string(),
            signature: signature.to_string(),
            func: f,
        });
        self
    }

    pub fn add_source_file(mut self, path: &str) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::IO {
            path: path.to_string(),
            source,
        })?;
        self.source_files.push((path.to_string(), text));
        Ok(self)
    }

    pub fn build(self) -> Result<Engine, Error> {
        let EngineBuilder {
            host_fns,
            source_files,
        } = self;

        let mut ast = Ast::new();
        let mut defs = Vec::new();

        for (id, (_, text)) in source_files.iter().enumerate() {
            let parsed_source = parse_file_into(text, id as u32, &mut ast).map_err(Error::Parse)?;
            defs.extend(parsed_source);
        }

        let mut checker = Checker::new(&mut ast);
        for host_fn in &host_fns {
            checker
                .declare_host_fn(&host_fn.name, &host_fn.signature)
                .map_err(Error::Type)?;
        }

        checker.check(&mut defs).map_err(Error::Type)?;

        let mut interpreter = Interpreter::new(Program { defs, ast });
        for host_fn in host_fns {
            interpreter.register_function(&host_fn.name, host_fn.func);
        }

        Ok(Engine {
            interpreter,
            source_files,
        })
    }
}

struct HostDecl {
    name: String,
    signature: String,
    func: fn(Span, &[Value]) -> EvalResult<Value>,
}

pub struct Engine {
    interpreter: Interpreter,
    source_files: Vec<(String, String)>,
}

impl Engine {
    pub fn global(&self, name: &str) -> Result<Value, Error> {
        let span = Span::new(HOST_FILE);

        if let Some(global) = self.interpreter.globals.get(name) {
            return Ok(global.clone());
        }

        if let Some(constant) = self.interpreter.constants.get(name) {
            let result = self
                .interpreter
                .force_constant(constant, name, span)
                .map_err(Error::Runtime)?;
            return Ok(result);
        }

        Err(Error::Unbound(name.to_string()))
    }

    pub fn call(&self, name: &str, args: &[Value]) -> Result<Value, Error> {
        let span = Span::new(HOST_FILE);

        let func = self.global(name)?;
        match func {
            Value::Closure(c) => {
                if c.params.len() != args.len() {
                    return Err(Error::Runtime(EvalError::ArityError {
                        span,
                        expected: c.params.len(),
                        got: args.len(),
                    }));
                }

                let mut env = c.env.clone();
                for (param, val) in c.params.iter().zip(args) {
                    env = env.extend(param.clone(), val.clone());
                }

                self.interpreter.eval(c.body, env).map_err(Error::Runtime)
            }

            Value::Constructor(c) => {
                if c.arity != args.len() {
                    return Err(Error::Runtime(EvalError::ArityError {
                        span,
                        expected: c.arity,
                        got: args.len(),
                    }));
                }

                Ok(Value::Variant(Rc::new(Variant {
                    name: c.name.clone(),
                    fields: args.to_vec(),
                })))
            }

            Value::HostFn(h) => h(span, args).map_err(Error::Runtime),

            other => Err(Error::Runtime(EvalError::NotCallableError {
                span,
                ty: other.type_name(),
            })),
        }
    }
}

pub enum Error {
    IO { path: String, source: io::Error },
    Unbound(String),
    Type(TypeError),
    Parse(ParseError),
    Runtime(EvalError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IO { path, source } => write!(f, "{}: {}", path, source),
            Self::Unbound(n) => write!(f, "No value named `{}` in scope.", n),
            Self::Parse(e) => write!(f, "{}", e),
            Self::Runtime(e) => write!(f, "{}", e),
            Self::Type(_) => write!(f, "type error"),
        }
    }
}
