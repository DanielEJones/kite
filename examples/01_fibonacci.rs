use kite::{EngineBuilder, Error, Value};

fn run() -> Result<Value, Error> {
    let engine = EngineBuilder::new()
        .add_source_text(
            "fib",
            r#"
                # Kite allows you to use normal functions as methods!
                # The first argument to a function becomes the receiver.
                def fib(n: num): num = n.fib_helper(0, 1);

                # This doesn't require any changes to the function being
                # used as a method; they are written exactly the same way.
                def fib_helper(n: num, a: num, b: num): num =
                  if n < 1 
                    then a
                    else fib_helper(n - 1, b, a + b);
            "#,
        )
        .build()?;

    engine.call("fib", &[Value::Num(10.0)])
}

fn main() {
    match run() {
        Ok(v) => println!("{}", v),
        Err(e) => println!("{}", e),
    }
}
