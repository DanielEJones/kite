mod checker;
mod engine;
mod error;
mod interpreter;
mod lexer;
mod parser;

use crate::{
    engine::{EngineBuilder, Error},
    interpreter::{EvalError, EvalErrorKind, EvalResult, Value},
    lexer::Span,
};

fn run() -> Result<Value, Error> {
    let engine = EngineBuilder::new()
        .add_source_file("example.txt")?
        .add_host_fn(
            "sqrt",
            "num -> num",
            host_fn!(span, [n] {
                let num = n.as_num(span)?;
                Ok(Value::Num(num.sqrt()))
            }),
        )
        .build()?;

    engine.call("main", &[Value::Num(1.0)])
}

fn main() {
    match run() {
        Ok(val) => println!("{}", val),
        Err(e) => eprintln!("{}", e),
    }
}
