use kite::{EngineBuilder, Error, Value};

fn run() -> Result<Value, Error> {
    let engine = EngineBuilder::new()
        .add_source_text(
            "records",
            r#"
                # Records are made with the record keyword and are enclosed in
                # curly braces. Each field gets a name and a type.
                type vector = record 
                  { x: num,
                    y: num,
                  };

                # A record `x` is given the constructor `mk_x`, as well as field
                # access functions `f: x -> fty` for each field.
                def add_vector(l: vector, r: vector): vector =
                  mk_vector(l.x + r.x, l.y + r.y);

                # In addition, records have an update function `with_f` that for
                # a given record returns a new record with that field set to the
                # desired value
                def my_point: vector = 
                  mk_vector(3, 4)
                    .add_vector(mk_vector(5, 6))
                    .with_x(-1);

                def my_total: num = my_point.x * my_point.y;
            "#,
        )
        .build()?;

    engine.global("my_total")
}

fn main() {
    match run() {
        Ok(v) => println!("{}", v),
        Err(e) => println!("{}", e),
    }
}
