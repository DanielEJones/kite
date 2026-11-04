use kite::{EngineBuilder, Error, Value};

fn run() -> Result<Value, Error> {
    let engine = EngineBuilder::new()
        .add_source_text(
            "datatypes",
            r#"
                # New types are introduced with the `type` keyword. The basic 
                # form of types are Algebraic Data Types, or ADTs; they consist
                # of a list of constructors describing how a value of the type
                # can be created.

                type list = 
                  | empty               # We can make an empty list from nothing
                  | append(list, num);  # And given a list we can add an element

                def sum(l: list): num =
                  match l 
                  | empty         => 0
                  \ append(xs, x) => xs.sum + x;

                # Constructors are like functions; they can be chained!
                def my_sum: num = 
                  empty.append(1).append(2).append(3).append(4)
                       .sum;
            "#,
        )
        .build()?;

    engine.global("my_sum")
}

fn main() {
    match run() {
        Ok(v) => println!("{}", v),
        Err(e) => println!("{}", e),
    }
}
