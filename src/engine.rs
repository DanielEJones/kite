use std::rc::Rc;

use crate::{
    checker::{Checker, TypeError},
    error::SourceMap,
    interpreter::{EvalError, EvalErrorKind, EvalResult, Interpreter, Value, Variant},
    lexer::{HOST_FILE, Span},
    parser::{Ast, Definition, Program, parse_file_into},
};

pub use crate::error::Error;

pub struct EngineBuilder {
    host_fns: Vec<HostDecl>,
    source_files: SourceMap,
}

impl EngineBuilder {
    pub fn new() -> EngineBuilder {
        EngineBuilder {
            host_fns: Vec::new(),
            source_files: SourceMap::new(),
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
        self.source_files.add(path, &text);
        Ok(self)
    }

    pub fn build(self) -> Result<Engine, Error> {
        let EngineBuilder {
            host_fns,
            source_files,
        } = self;

        let mut ast = Ast::new();
        let mut defs = Vec::new();

        for (id, text) in source_files.texts().enumerate() {
            let parsed_source = parse_file_into(text, id as u32, &mut ast)
                .map_err(|e| source_files.parse_error(&e))?;
            defs.extend(parsed_source);
        }

        check(&mut ast, &mut defs, &host_fns).map_err(|e| source_files.type_error(&e, &ast))?;

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

fn check(
    ast: &mut Ast,
    defs: &mut Vec<Definition>,
    host_fns: &[HostDecl],
) -> Result<(), TypeError> {
    let mut checker = Checker::new(ast);
    for h in host_fns {
        checker.declare_host_fn(&h.name, &h.signature)?;
    }
    checker.check(defs)
}

struct HostDecl {
    name: String,
    signature: String,
    func: fn(Span, &[Value]) -> EvalResult<Value>,
}

pub struct Engine {
    interpreter: Interpreter,
    source_files: SourceMap,
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
                .map_err(|e| self.runtime_error(e))?;
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
                    return Err(self.runtime_error(EvalError {
                        span,
                        kind: EvalErrorKind::ArityError {
                            expected: c.params.len(),
                            got: args.len(),
                        },
                    }));
                }

                let mut env = c.env.clone();
                for (param, val) in c.params.iter().zip(args) {
                    env = env.extend(param.clone(), val.clone());
                }

                self.interpreter
                    .eval(c.body, env)
                    .map_err(|e| self.runtime_error(e))
            }

            Value::Constructor(c) => {
                if c.arity != args.len() {
                    return Err(self.runtime_error(EvalError {
                        span,
                        kind: EvalErrorKind::ArityError {
                            expected: c.arity,
                            got: args.len(),
                        },
                    }));
                }

                Ok(Value::Variant(Rc::new(Variant {
                    name: c.name.clone(),
                    fields: args.to_vec(),
                })))
            }

            Value::HostFn(h) => h(span, args).map_err(|e| self.runtime_error(e)),

            other => Err(self.runtime_error(EvalError {
                span,
                kind: EvalErrorKind::NotCallableError {
                    ty: other.type_name(),
                },
            })),
        }
    }

    fn runtime_error(&self, e: EvalError) -> Error {
        self.source_files.runtime_error(&e)
    }
}
