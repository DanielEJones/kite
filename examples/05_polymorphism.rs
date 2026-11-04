use kite::{EngineBuilder, Error, Value};

fn run() -> Result<Value, Error> {
    let engine = EngineBuilder::new()
        .add_source_text(
            "polymorphism",
            r#"
                type option(a) 
                  = some(a)
                  | none;

                type nonempty_list(a) = record 
                  { elem: a,
                    rest: option(nonempty_list(a)),
                  };

                def cons(@a: type, head: a, tail: nonempty_list(a)): nonempty_list(a) =
                  mk_nonempty_list(head, tail.some);

                def one(@a: type, head: a): nonempty_list(a) =
                  mk_nonempty_list(head, none);

                def fold(@a: type, xs: nonempty_list(a), f: (a, a) -> a): a =
                  match xs.rest 
                  | none       => xs.elem
                  \ some(rest) => f(xs.elem, rest.fold(f));


                def a_list: nonempty_list(num) = cons(1, cons(2, one(3)));

                def a_num: num = a_list.fold(fun (a, b) => a + b);
            "#,
        )
        .build()?;

    engine.global("a_num")
}

fn main() {
    match run() {
        Ok(v) => println!("{}", v),
        Err(e) => println!("{}", e),
    }
}
