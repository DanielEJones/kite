use kite::{EngineBuilder, Error, Value};

fn run() -> Result<Value, Error> {
    let engine = EngineBuilder::new()
        .add_source_text(
            "fact",
            r#"
                def fact(n: num): num =
                  if n < 1 
                    then 1
                    else n * (n - 1).fact;
            "#,
        )
        .build()?;

    engine.call("fact", &[Value::Num(10.0)])
}

fn main() {
    match run() {
        Ok(v) => println!("{}", v),
        Err(e) => println!("{}", e),
    }
}
