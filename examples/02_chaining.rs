use kite::{EngineBuilder, Error, Value};

fn run() -> Result<Value, Error> {
    let engine = EngineBuilder::new()
        .add_source_text(
            "chains",
            r#"
                # Method chaining allows for values to flow through a pipeline
                # of calls without wrapping the entire expression in parenthesis
                def my_number: num = 
                  100.mul(3)
                     .div(4)
                     .sub(5)
                     .add(6);

                def add(a: num, b: num): num = a + b;
                def sub(a: num, b: num): num = a - b;

                def mul(a: num, b: num): num = a * b;
                def div(a: num, b: num): num = a / b;
            "#,
        )
        .build()?;

    engine.global("my_number")
}

fn main() {
    match run() {
        Ok(v) => println!("{}", v),
        Err(e) => println!("{}", e),
    }
}
