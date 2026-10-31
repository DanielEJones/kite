use crate::{
    lexer::Span,
    parser::{ConstDef, Definition, FnDef, Node, NodeId, Program, TypeDef, TypeDefBody, TypeId},
};
use core::fmt;
use std::{collections::HashMap, rc::Rc};

pub struct Interpreter {
    program: Program,
    globals: HashMap<String, Value>,
}

impl Interpreter {
    pub fn new(program: Program) -> Interpreter {
        let mut globals = HashMap::new();

        for (name, f) in builtins() {
            globals.insert(name.to_string(), Value::Builtin(name, *f));
        }

        for def in &program.defs {
            match def {
                Definition::Function(FnDef {
                    name, params, body, ..
                }) => {
                    globals.insert(
                        name.clone(),
                        Value::Closure(Rc::new(Closure {
                            params: params.clone(),
                            body: *body,
                            env: Env::Nil,
                        })),
                    );
                }

                Definition::Type(TypeDef { body, name, .. }) => match body {
                    TypeDefBody::ADT(arms) => {
                        for variant in arms {
                            make_variant(&variant.name, &variant.args, &mut globals);
                        }
                    }

                    TypeDefBody::Record(fields) => {
                        let args = fields.iter().map(|(_, ty)| *ty).collect();
                        make_variant(&format!("mk_{}", name), &args, &mut globals);

                        // make the method bodies here
                    }
                },

                Definition::Constant(_) => {}
            }
        }

        let mut interpreter = Interpreter { program, globals };

        for def in &interpreter.program.defs {
            if let Definition::Constant(ConstDef { name, body, .. }) = def {
                let value = interpreter.eval(*body, Env::Nil).unwrap();
                interpreter.globals.insert(name.clone(), value);
            };
        }

        interpreter
    }

    pub fn run(&self, entry: &str, args: Vec<Value>) -> EvalResult<Value> {
        let function = self
            .program
            .defs
            .iter()
            .filter_map(|d| match d {
                Definition::Function(f) => Some(f),
                _ => None,
            })
            .find(|f| f.name == entry)
            .expect("Couldn't find the entry point");

        if args.len() != function.params.len() {
            return Err(EvalError::ArityError {
                span: function.span,
                expected: function.params.len(),
                got: args.len(),
            });
        }

        let mut root_env = Env::Nil;

        for (param, arg) in function.params.iter().zip(args) {
            root_env = root_env.extend(param.clone(), arg);
        }

        self.eval(function.body, root_env)
    }

    fn eval(&self, mut node: NodeId, mut env: Env) -> EvalResult<Value> {
        loop {
            match *self.program.ast.get_node(node) {
                Node::Num(n) => {
                    return Ok(Value::Num(n));
                }
                Node::Bool(b) => {
                    return Ok(Value::Bool(b));
                }
                Node::Var(ref v) => {
                    return env
                        .lookup(v)
                        .or_else(|| self.globals.get(v).cloned())
                        .ok_or_else(|| EvalError::Unbound {
                            span: *self.program.ast.get_node_span(node),
                            var: v.clone(),
                        });
                }
                Node::Add(l, r) => {
                    return self.arith(l, r, &env, |a, b| a + b);
                }
                Node::Sub(l, r) => {
                    return self.arith(l, r, &env, |a, b| a - b);
                }
                Node::Mul(l, r) => {
                    return self.arith(l, r, &env, |a, b| a * b);
                }
                Node::Div(l, r) => {
                    return self.arith(l, r, &env, |a, b| a / b);
                }
                Node::Neg(v) => {
                    return Ok(Value::Num(
                        -(self
                            .eval(v, env)?
                            .as_num(*self.program.ast.get_node_span(node))?),
                    ));
                }
                Node::Lt(l, r) => {
                    return self.comp(l, r, &env, |a, b| a < b);
                }
                Node::Le(l, r) => {
                    return self.comp(l, r, &env, |a, b| a <= b);
                }
                Node::Gt(l, r) => {
                    return self.comp(l, r, &env, |a, b| a > b);
                }
                Node::Ge(l, r) => {
                    return self.comp(l, r, &env, |a, b| a >= b);
                }
                Node::Eq(l, r) => {
                    return Ok(Value::Bool(
                        self.eval(l, env.clone())?
                            .equals(&self.eval(r, env.clone())?),
                    ));
                }
                Node::Ne(l, r) => {
                    return Ok(Value::Bool(
                        !self
                            .eval(l, env.clone())?
                            .equals(&self.eval(r, env.clone())?),
                    ));
                }
                Node::And(l, r) => {
                    let a = self
                        .eval(l, env.clone())?
                        .as_bool(*self.program.ast.get_node_span(l))?;

                    if !a {
                        return Ok(Value::Bool(false));
                    }

                    return Ok(self.eval(r, env)?);
                }
                Node::Or(l, r) => {
                    let a = self
                        .eval(l, env.clone())?
                        .as_bool(*self.program.ast.get_node_span(l))?;

                    if a {
                        return Ok(Value::Bool(true));
                    }

                    return Ok(self.eval(r, env)?);
                }
                Node::Not(v) => {
                    return Ok(Value::Bool(
                        !(self
                            .eval(v, env)?
                            .as_bool(*self.program.ast.get_node_span(v))?),
                    ));
                }
                Node::Lam(ref params, body) => {
                    return Ok(Value::Closure(Rc::new(Closure {
                        params: params.clone(),
                        body: body,
                        env: env.clone(),
                    })));
                }
                Node::Let(ref name, value, k) => {
                    let v = self.eval(value, env.clone())?;
                    env = env.extend(name.clone(), v);
                    node = k;
                }
                Node::If(cond, then, els) => {
                    node = match self.eval(cond, env.clone())? {
                        Value::Bool(true) => then,
                        Value::Bool(false) => els,
                        other => {
                            return Err(EvalError::TypeError {
                                span: *self.program.ast.get_node_span(cond),
                                expected: "boolean",
                                got: other.type_name(),
                            });
                        }
                    }
                }
                Node::Match(target, ref arms) => {
                    let variant = self
                        .eval(target, env.clone())?
                        .as_variant(*self.program.ast.get_node_span(target))?;

                    let arm = arms
                        .iter()
                        .find(|a| a.variant == variant.name)
                        .ok_or_else(|| EvalError::NoMatchError {
                            span: *self.program.ast.get_node_span(target),
                            variant: variant.name.clone(),
                        })?;

                    if arm.binders.len() != variant.fields.len() {
                        return Err(EvalError::ArityError {
                            span: *self.program.ast.get_node_span(arm.body),
                            expected: variant.fields.len(),
                            got: arm.binders.len(),
                        });
                    }

                    for (binder, field) in arm.binders.iter().zip(&variant.fields) {
                        env = env.extend(binder.clone(), field.clone());
                    }

                    node = arm.body
                }
                Node::Call(fun, ref args) => {
                    let f = self.eval(fun, env.clone())?;
                    let args = args
                        .iter()
                        .map(|&a| self.eval(a, env.clone()))
                        .collect::<EvalResult<Vec<_>>>()?;

                    match f {
                        Value::Closure(c) => {
                            if c.params.len() != args.len() {
                                return Err(EvalError::ArityError {
                                    span: *self.program.ast.get_node_span(node),
                                    expected: c.params.len(),
                                    got: args.len(),
                                });
                            }

                            let mut call_env = c.env.clone();
                            for (param, val) in c.params.iter().zip(args) {
                                call_env = call_env.extend(param.clone(), val);
                            }
                            env = call_env;
                            node = c.body;
                        }
                        Value::Builtin(_, f) => {
                            return f(*self.program.ast.get_node_span(node), &args);
                        }
                        Value::Constructor(c) => {
                            if c.arity != args.len() {
                                return Err(EvalError::ArityError {
                                    span: *self.program.ast.get_node_span(node),
                                    expected: c.arity,
                                    got: args.len(),
                                });
                            }
                            return Ok(Value::Variant(Rc::new(Variant {
                                name: c.name.clone(),
                                fields: args,
                            })));
                        }
                        other => {
                            return Err(EvalError::NotCallableError {
                                span: *self.program.ast.get_node_span(fun),
                                ty: other.type_name(),
                            });
                        }
                    }
                }
            }
        }
    }

    fn arith<F>(&self, left: NodeId, right: NodeId, env: &Env, op: F) -> EvalResult<Value>
    where
        F: FnOnce(f64, f64) -> f64,
    {
        let a = self
            .eval(left, env.clone())?
            .as_num(*self.program.ast.get_node_span(left))?;
        let b = self
            .eval(right, env.clone())?
            .as_num(*self.program.ast.get_node_span(right))?;
        Ok(Value::Num(op(a, b)))
    }

    fn comp<F>(&self, left: NodeId, right: NodeId, env: &Env, op: F) -> EvalResult<Value>
    where
        F: FnOnce(f64, f64) -> bool,
    {
        let a = self
            .eval(left, env.clone())?
            .as_num(*self.program.ast.get_node_span(left))?;
        let b = self
            .eval(right, env.clone())?
            .as_num(*self.program.ast.get_node_span(right))?;
        Ok(Value::Bool(op(a, b)))
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Num(f64),
    Bool(bool),
    Variant(Rc<Variant>),
    Closure(Rc<Closure>),
    Builtin(&'static str, fn(Span, &[Value]) -> EvalResult<Value>),
    Constructor(Rc<Constructor>),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            Value::Num(n) => {
                write!(f, "(number {})", n)
            }
            Value::Bool(b) => {
                write!(f, "(boolean {})", b)
            }
            Value::Variant(v) => {
                write!(f, "({}", v.name)?;
                for field in &v.fields {
                    write!(f, " {}", field)?;
                }
                write!(f, ")")
            }
            Value::Closure(_) => {
                write!(f, "(closure)")
            }
            Value::Builtin(name, _) => {
                write!(f, "(builtin {})", name)
            }
            Value::Constructor(c) => {
                write!(f, "(constructor {})", c.name)
            }
        }
    }
}

impl Value {
    fn as_num(&self, span: Span) -> EvalResult<f64> {
        match *self {
            Value::Num(n) => Ok(n),
            ref other => Err(EvalError::TypeError {
                span,
                expected: "number",
                got: other.type_name(),
            }),
        }
    }

    fn as_bool(&self, span: Span) -> EvalResult<bool> {
        match *self {
            Value::Bool(b) => Ok(b),
            ref other => Err(EvalError::TypeError {
                span,
                expected: "boolean",
                got: other.type_name(),
            }),
        }
    }

    fn as_variant(&self, span: Span) -> EvalResult<Rc<Variant>> {
        match self {
            Value::Variant(v) => Ok(v.clone()),
            other => Err(EvalError::TypeError {
                span,
                expected: "variant",
                got: other.type_name(),
            }),
        }
    }

    fn type_name(&self) -> &'static str {
        match *self {
            Value::Num(_) => "number",
            Value::Bool(_) => "boolean",
            Value::Variant(_) => "variant",
            Value::Closure(_) | Value::Builtin(_, _) | Value::Constructor(_) => "function",
        }
    }

    fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Num(l), Value::Num(r)) => l == r,
            (Value::Bool(l), Value::Bool(r)) => l == r,
            _ => false,
        }
    }
}

fn make_variant(name: &String, args: &Vec<TypeId>, defs: &mut HashMap<String, Value>) {
    defs.insert(
        name.clone(),
        if args.is_empty() {
            Value::Variant(Rc::new(Variant {
                name: name.clone(),
                fields: Vec::new(),
            }))
        } else {
            Value::Constructor(Rc::new(Constructor {
                name: name.clone(),
                arity: args.len(),
            }))
        },
    );
}

#[derive(Debug)]
pub struct Variant {
    name: String,
    fields: Vec<Value>,
}

#[derive(Debug)]
pub struct Closure {
    params: Vec<String>,
    body: NodeId,
    env: Env,
}

#[derive(Debug)]
pub struct Constructor {
    name: String,
    arity: usize,
}

#[derive(Debug, Clone)]
enum Env {
    Nil,
    Cons {
        name: String,
        value: Value,
        parent: Rc<Env>,
    },
}

impl Env {
    fn lookup(&self, v: &str) -> Option<Value> {
        let mut current = self;
        loop {
            match current {
                Env::Nil => return None,
                Env::Cons {
                    name,
                    value,
                    parent,
                } if name == v => return Some(value.clone()),
                Env::Cons {
                    name: _,
                    value: _,
                    parent,
                } => current = parent,
            }
        }
    }

    fn extend(&self, name: String, value: Value) -> Env {
        Env::Cons {
            name,
            value,
            parent: Rc::new(self.clone()),
        }
    }
}

type EvalResult<T> = Result<T, EvalError>;

#[derive(Debug)]
pub enum EvalError {
    Unbound {
        span: Span,
        var: String,
    },
    TypeError {
        span: Span,
        expected: &'static str,
        got: &'static str,
    },
    ArityError {
        span: Span,
        expected: usize,
        got: usize,
    },
    NotCallableError {
        span: Span,
        ty: &'static str,
    },
    NoMatchError {
        span: Span,
        variant: String,
    },
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            EvalError::Unbound { span, var } => {
                write!(f, "{}: Unbound variable `{}`", span, var)
            }
            EvalError::ArityError {
                span,
                expected,
                got,
            } => {
                write!(
                    f,
                    "{}: Unexpected argument count, expected {} but got {}",
                    span, expected, got
                )
            }
            EvalError::TypeError {
                span,
                expected,
                got,
            } => {
                write!(
                    f,
                    "{}: Unexpected type, expected `{}` but got `{}`",
                    span, expected, got
                )
            }
            EvalError::NotCallableError { span, ty } => {
                write!(f, "{}: Cannot call value of type `{}`", span, ty)
            }
            EvalError::NoMatchError { span, variant } => {
                write!(f, "{}: No match found for variant `{}`", span, variant)
            }
        }
    }
}

fn builtins() -> &'static [(&'static str, fn(Span, &[Value]) -> EvalResult<Value>)] {
    &[("lt", |span, a| binary(span, a, |x, y| Value::Bool(x < y)))]
}

fn binary<F>(span: Span, args: &[Value], f: F) -> EvalResult<Value>
where
    F: FnOnce(f64, f64) -> Value,
{
    match args {
        [a, b] => Ok(f(a.as_num(span)?, b.as_num(span)?)),
        _ => Err(EvalError::ArityError {
            span,
            expected: 2,
            got: args.len(),
        }),
    }
}
