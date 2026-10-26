use std::fs::read_to_string;

mod lexer;
use lexer::Lexer;

mod parser;
use parser::Parser;

mod checker;
use checker::check_program;

mod interpreter;
use interpreter::Interpreter;

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

    if let Err(e) = check_program(&mut program) {
        eprintln!("{}", e.render(&program.ast));
        return;
    }

    let interpreter = Interpreter::new(program);
    let result = match interpreter.run("main", vec![Value::Num(1.0)]) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    println!("{}", result);
}
