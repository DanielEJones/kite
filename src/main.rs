use std::fs::read_to_string;

mod lexer;
use lexer::{Lexer, Span};

mod parser;
use parser::Parser;

mod checker;
use checker::Checker;

mod interpreter;
use interpreter::{EvalError, EvalResult, Interpreter};

use crate::interpreter::Value;

fn main() {
    let source = read_to_string("example.txt").expect("Failed to open file");
    let lexer = Lexer::new(source);

    let parser = Parser::new(lexer);
    let mut program = match parser.parse() {
        Ok(tree) => tree,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    let mut checker = Checker::new(&mut program.ast);
    checker.declare_host_fn("sqrt", "num -> num").unwrap();

    if let Err(e) = checker.check(&mut program.defs) {
        eprintln!("{}", e.render(&program.ast));
        return;
    }

    let mut interpreter = Interpreter::new(program);
    interpreter.register_function(
        "sqrt",
        host_fn!(span, [n] {
            let num = n.as_num(span)?;
            Ok(Value::Num(num.sqrt()))
        }),
    );

    let result = match interpreter.run("main", vec![Value::Num(1.0)]) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    println!("{}", result);
}
