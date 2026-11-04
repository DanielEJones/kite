use kite::{EngineBuilder, Error, Value, host_fn};

fn run() -> Result<Value, Error> {
    let engine = EngineBuilder::new()
        .add_source_text(
            "host_functions",
            r#"
                type vec2 = record 
                  { x: num,
                    y: num,
                  };

                def add(v1: vec2, v2: vec2): vec2 =
                  let new_x := v1.x + v2.x;
                  let new_y := v1.y + v2.y;
                  mk_vec2(new_x, new_y);

                def scale(v: vec2, k: num): vec2 = 
                  let new_x := v.x * k;
                  let new_y := v.y * k;
                  mk_vec2(new_x, new_y);

                def neg(v: vec2): vec2 =
                  let new_x := -v.x;
                  let new_y := -v.y;
                  mk_vec2(new_x, new_y);

                def len(v: vec2): num = 
                  let dist_squared := v.x * v.x + v.y * v.y;
                  dist_squared.sqrt;


                def my_num: num = 
                  mk_vec2(1, 2)
                    .add(mk_vec2(2, 2))
                    .scale(6)
                    .neg
                    .len;
            "#,
        )
        .add_host_fn(
            "sqrt",
            "num -> num",
            host_fn!(span, [n] {
                let num = n.as_num(span)?;
                Ok(Value::Num(num.sqrt()))
            }),
        )
        .build()?;

    engine.global("my_num")
}

fn main() {
    match run() {
        Ok(val) => println!("{}", val),
        Err(e) => eprintln!("{}", e),
    }
}
