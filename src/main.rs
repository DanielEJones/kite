mod checker;
mod engine;
mod interpreter;
mod lexer;
mod parser;

use crate::{
    engine::{EngineBuilder, Error},
    interpreter::{EvalError, EvalResult, Value},
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

    engine.call("maint", &[Value::Num(1.0)])
}

fn main() {
    match run() {
        Ok(val) => println!("{}", val),
        Err(e) => eprintln!("{}", e),
    }
    // let source = read_to_string("example.txt").expect("Failed to open file");
    // let lexer = Lexer::new(source, 0);

    // let mut parser = Parser::new(lexer);
    // let mut program = match parser.parse() {
    //     Ok(tree) => tree,
    //     Err(e) => {
    //         eprintln!("{}", e);
    //         return;
    //     }
    // };

    // let mut checker = Checker::new(&mut parser.tree);
    // checker.declare_host_fn("sqrt", "num -> num").unwrap();

    // if let Err(e) = checker.check(&mut program) {
    //     eprintln!("{}", e.render(&parser.tree));
    //     return;
    // }

    // let mut interpreter = Interpreter::new(Program {
    //     ast: parser.tree,
    //     defs: program,
    // });

    // interpreter.register_function(
    //     "sqrt",
    //     host_fn!(span, [n] {
    //         let num = n.as_num(span)?;
    //         Ok(Value::Num(num.sqrt()))
    //     }),
    // );

    // let result = match interpreter.run("main", vec![Value::Num(1.0)]) {
    //     Ok(v) => v,
    //     Err(e) => {
    //         eprintln!("{}", e);
    //         return;
    //     }
    // };

    // println!("{}", result);
}
