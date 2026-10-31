use std::collections::HashMap;

use crate::{
    lexer::Span,
    parser::{
        Arm, Ast, Definition, FnDef, Node, NodeId, Type, TypeDef, TypeDefBody, TypeId,
        parse_type_into,
    },
};

pub struct Checker<'a> {
    ast: &'a mut Ast,
    adts: HashMap<String, Adt>,
    env: Vec<(String, TypeId)>,
    ty_vars: Vec<String>,
}

#[derive(Clone)]
struct Adt {
    vars: Vec<String>,
    ctors: Vec<(String, Vec<TypeId>)>,
}

#[derive(Clone, Copy)]
struct Scope {
    env: usize,
    ty_vars: usize,
}

type Solved = HashMap<String, TypeId>;

impl<'a> Checker<'a> {
    pub fn new(ast: &'a mut Ast) -> Checker<'a> {
        Checker {
            ast,
            adts: HashMap::new(),
            env: Vec::new(),
            ty_vars: Vec::new(),
        }
    }

    pub fn check(&mut self, defs: &mut Vec<Definition>) -> Result<(), TypeError> {
        self.desugar_records(defs);
        self.check_defs(&defs)?;
        Ok(())
    }

    fn desugar_records(&mut self, defs: &mut Vec<Definition>) {
        let mut generated = Vec::new();

        for def in defs.iter() {
            let Definition::Type(td) = def else {
                continue;
            };
            let TypeDefBody::Record(fields) = &td.body else {
                continue;
            };

            let span = td.span;
            let ctor = format!("mk_{}", td.name);
            let binders: Vec<String> = (0..fields.len()).map(|i| format!("_{}", i)).collect();
            let rec_ty = self.type_app(&td.name, &td.vars, span);

            for (i, (field_name, field_ty)) in fields.iter().enumerate() {
                // Make the projection function `<field_name> : <rec> -> <field>`
                let field_var_node = Node::Var(binders[i].clone());
                let field_var = self.ast.add_node(field_var_node, span);
                let fn_body = self.match_record("r", &ctor, &binders, field_var, span);

                let proj_type_node = Type::Function(vec![rec_ty], *field_ty);
                let proj_typ = self.ast.add_type(proj_type_node, span);
                let fn_type = self.forall(&td.vars, proj_typ, span);

                generated.push(Definition::Function(FnDef {
                    span,
                    name: field_name.clone(),
                    params: vec!["r".to_string()],
                    typ: fn_type,
                    body: fn_body,
                }));

                // Make the update function `with_<field_name> : (<rec>, <field>) -> <rec>`
                //
                // Any type variables that appear only in the updated field may change type,
                // so seach the other fields to see if the var occurs or not and take that
                // fresh var if not.

                let own_vars = self.unsolved(*field_ty, &td.vars, &Solved::new());

                let mut other_vars = Vec::new();
                for (_, (_, other_ty)) in fields.iter().enumerate().filter(|(j, _)| *j != i) {
                    self.collect_unsolved(*other_ty, &td.vars, &Solved::new(), &mut other_vars);
                }

                let renamed: Vec<String> = own_vars
                    .into_iter()
                    .filter(|v| !other_vars.contains(v))
                    .collect();

                let mut map = Solved::new();
                let mut new_vars = Vec::new();
                for v in &renamed {
                    let fresh_var = format!("{}'", v);

                    let fresh_typ_node = Type::Named(fresh_var.clone());
                    let fresh_typ = self.ast.add_type(fresh_typ_node, span);

                    map.insert(v.clone(), fresh_typ);
                    new_vars.push(fresh_var);
                }

                let new_field_ty = self.substitute(*field_ty, &map);
                let new_rec_args: Vec<TypeId> = td
                    .vars
                    .iter()
                    .map(|v| match map.get(v) {
                        Some(t) => *t,
                        None => self.ast.add_type(Type::Named(v.clone()), span),
                    })
                    .collect();

                let new_rec_ty = self.ast.add_type(
                    if new_rec_args.is_empty() {
                        Type::Named(td.name.clone())
                    } else {
                        Type::Applied(td.name.clone(), new_rec_args)
                    },
                    span,
                );

                let new_ctor_node = Node::Var(ctor.clone());
                let new_ctor = self.ast.add_node(new_ctor_node, span);

                let new_args: Vec<NodeId> = binders
                    .iter()
                    .enumerate()
                    .map(|(j, name)| {
                        let name = if j == i {
                            "v".to_string()
                        } else {
                            name.clone()
                        };
                        let arg_node = Node::Var(name);
                        self.ast.add_node(arg_node, span)
                    })
                    .collect();
                let new_call_node = Node::Call(new_ctor, new_args);
                let new_call = self.ast.add_node(new_call_node, span);
                let fn_body = self.match_record("r", &ctor, &binders, new_call, span);

                let update_type_node = Type::Function(vec![rec_ty, new_field_ty], new_rec_ty);
                let update_type = self.ast.add_type(update_type_node, span);
                let all_vars: Vec<String> = td.vars.iter().cloned().chain(new_vars).collect();
                let fn_type = self.forall(&all_vars, update_type, span);

                generated.push(Definition::Function(FnDef {
                    span,
                    name: format!("with_{}", field_name),
                    params: vec!["r".to_string(), "v".to_string()],
                    typ: fn_type,
                    body: fn_body,
                }));
            }
        }

        defs.extend(generated);
    }

    fn check_defs(&mut self, defs: &[Definition]) -> Result<(), TypeError> {
        // Register every ADT so types can refer to each other in any order
        for def in defs {
            if let Definition::Type(td) = def {
                let adt = Adt {
                    vars: td.vars.clone(),
                    ctors: match td.body {
                        // ADT variants are as-is
                        TypeDefBody::ADT(ref arms) => arms
                            .iter()
                            .map(|arm| (arm.name.clone(), arm.args.clone()))
                            .collect(),

                        // synthesise one constructor for records
                        TypeDefBody::Record(ref fields) => {
                            vec![(
                                format!("mk_{}", &td.name),
                                fields.iter().map(|(_, ty)| *ty).collect(),
                            )]
                        }
                    },
                };
                if self.adts.insert(td.name.clone(), adt).is_some() {
                    return Err(err(
                        td.span,
                        TypeErrorKind::DuplicateDefinition(td.name.clone()),
                    ));
                }
            }
        }

        // Declare all the constructors for the types
        for def in defs {
            if let Definition::Type(td) = def {
                self.declare_ctors(td)?;
            }
        }

        // Declare the signatures of functions and constants
        for def in defs {
            let (name, typ, span) = match def {
                Definition::Function(f) => (&f.name, f.typ, f.span),
                Definition::Constant(c) => (&c.name, c.typ, c.span),
                Definition::Type(_) => continue,
            };
            self.check_type(typ)?;
            self.declare(name, typ, span)?;
        }

        // Check bodies of functions and constatnts against declarations
        for def in defs {
            match def {
                Definition::Function(f) => {
                    let (generics, fn_ty) = self.split_forall(f.typ);
                    let Type::Function(param_tys, ret) = self.ty(fn_ty) else {
                        return Err(err(f.span, TypeErrorKind::NotAFunction(f.typ)));
                    };

                    let scope = self.scope();
                    self.ty_vars.extend(generics);
                    let result = self.lam_body(f.span, &f.params, &param_tys, f.body, Some(ret));
                    self.restore(scope);
                    result?;
                }
                Definition::Constant(c) => self.check_expr(c.body, c.typ)?,
                Definition::Type(_) => {}
            }
        }

        Ok(())
    }

    fn declare_ctors(&mut self, td: &TypeDef) -> Result<(), TypeError> {
        let span = td.span;

        let scope = self.scope();
        self.ty_vars.extend(td.vars.iter().cloned());

        match td.body {
            TypeDefBody::ADT(ref arms) => {
                for arm in arms {
                    for arg in &arm.args {
                        self.check_type(*arg)?;
                    }
                }

                self.restore(scope);
                for arm in arms {
                    self.declare_ctor(&td.name, &arm.name, span, &td.vars, &arm.args)?;
                }
            }

            TypeDefBody::Record(ref fields) => {
                for (_, field_ty) in fields {
                    self.check_type(*field_ty)?;
                }
                self.restore(scope);

                let field_types = fields.iter().map(|(_, ty)| *ty).collect();
                self.declare_ctor(
                    &td.name,
                    &format!("mk_{}", td.name),
                    span,
                    &td.vars,
                    &field_types,
                )?;
            }
        }

        Ok(())
    }

    fn declare_ctor(
        &mut self,
        t_name: &str,
        c_name: &str,
        span: Span,
        vars: &Vec<String>,
        args: &Vec<TypeId>,
    ) -> Result<(), TypeError> {
        // Create the return type of the constructor
        let result = if vars.is_empty() {
            Type::Named(t_name.to_string())
        } else {
            let vars = vars
                .iter()
                .map(|v| self.ast.add_type(Type::Named(v.clone()), span))
                .collect();
            Type::Applied(t_name.to_string(), vars)
        };
        let result = self.ast.add_type(result, span);

        // Create the type of the constructor
        let mut ty = if args.is_empty() {
            result
        } else {
            self.ast
                .add_type(Type::Function(args.clone(), result), span)
        };

        if !vars.is_empty() {
            ty = self.ast.add_type(Type::Forall(vars.clone(), ty), span)
        }

        self.declare(c_name, ty, span)
    }

    pub fn declare_host_fn(&mut self, name: &str, ty: &str) -> Result<(), TypeError> {
        let ty = parse_type_into(ty, &mut self.ast)
            .unwrap_or_else(|e| panic!("bad signature for host function `{}`: {}", name, e));
        let span = *self.ast.get_type_span(ty);
        self.declare(name, ty, span)
    }

    fn declare(&mut self, name: &str, ty: TypeId, span: Span) -> Result<(), TypeError> {
        if self.env.iter().any(|(n, _)| n == name) {
            return Err(err(
                span,
                TypeErrorKind::DuplicateDefinition(name.to_string()),
            ));
        }
        self.env.push((name.to_string(), ty));
        Ok(())
    }

    fn infer(&mut self, expr: NodeId) -> Result<TypeId, TypeError> {
        self.visit(expr, None)
    }

    fn check_expr(&mut self, expr: NodeId, expected: TypeId) -> Result<(), TypeError> {
        self.visit(expr, Some(expected)).map(|_| ())
    }

    fn visit(&mut self, expr: NodeId, goal: Option<TypeId>) -> Result<TypeId, TypeError> {
        let span = self.node_span(expr);

        let ty = match self.node(expr) {
            Node::Num(_) => self.prim("num", span),
            Node::Bool(_) => self.prim("bool", span),

            Node::Var(name) => match self.lookup(&name) {
                Some(ty) => ty,
                None => return Err(err(span, TypeErrorKind::UnboundVariable(name))),
            },

            Node::Add(l, r) | Node::Sub(l, r) | Node::Mul(l, r) | Node::Div(l, r) => {
                let num = self.prim("num", span);
                self.check_expr(l, num)?;
                self.check_expr(r, num)?;
                num
            }

            Node::Neg(t) => {
                let num = self.prim("num", span);
                self.check_expr(t, num)?;
                num
            }

            Node::Lt(l, r) | Node::Le(l, r) | Node::Gt(l, r) | Node::Ge(l, r) => {
                let num = self.prim("num", span);
                self.check_expr(l, num)?;
                self.check_expr(r, num)?;
                self.prim("bool", span)
            }

            Node::And(l, r) | Node::Or(l, r) => {
                let bool = self.prim("bool", span);
                self.check_expr(l, bool)?;
                self.check_expr(r, bool)?;
                bool
            }

            Node::Not(t) => {
                let bool = self.prim("bool", span);
                self.check_expr(t, bool)?;
                bool
            }

            Node::Eq(l, r) | Node::Ne(l, r) => {
                let ty = self.infer(l)?;
                self.check_expr(r, ty)?;
                if !self.ty_comparable(ty) {
                    return Err(err(span, TypeErrorKind::NotComparable(ty)));
                }
                self.prim("bool", span)
            }

            Node::Lam(params, body) => self.visit_lam(expr, span, &params, body, goal)?,

            Node::Call(callee, args) => self.visit_call(span, callee, &args, goal)?,

            Node::Let(name, value, body) => {
                let value_ty = match self.annotation(value) {
                    Some(ann) => {
                        self.check_type(ann)?;
                        self.check_expr(value, ann)?;
                        ann
                    }
                    None => self.infer(value)?,
                };

                let scope = self.scope();
                self.bind(name, value_ty);
                let result = self.visit(body, goal);
                self.restore(scope);
                result?
            }

            Node::If(cond, then, els) => {
                let bool = self.prim("bool", span);
                self.check_expr(cond, bool)?;
                match goal {
                    Some(goal) => {
                        self.check_expr(then, goal)?;
                        self.check_expr(els, goal)?;
                        goal
                    }
                    None => {
                        let ty = self.infer(then)?;
                        self.check_expr(els, ty)?;
                        ty
                    }
                }
            }

            Node::Match(scrutinee, arms) => {
                let scrut_ty = self.infer(scrutinee)?;
                let Some(variants) = self.variants_of(scrut_ty) else {
                    return Err(err(span, TypeErrorKind::NotMatchable(scrut_ty)));
                };

                let mut seen: Vec<String> = Vec::new();
                let mut result_ty = goal;

                for arm in &arms {
                    let arm_span = self.node_span(arm.body);

                    let Some((_, fields)) = variants.iter().find(|(name, _)| *name == arm.variant)
                    else {
                        return Err(err(
                            arm_span,
                            TypeErrorKind::UnknownVariant {
                                variant: arm.variant.clone(),
                                ty: scrut_ty,
                            },
                        ));
                    };

                    if seen.contains(&arm.variant) {
                        return Err(err(
                            arm_span,
                            TypeErrorKind::DuplicateArm(arm.variant.clone()),
                        ));
                    }
                    seen.push(arm.variant.clone());

                    if arm.binders.len() != fields.len() {
                        return Err(err(
                            arm_span,
                            TypeErrorKind::ArityMismatch {
                                expected: fields.len(),
                                got: arm.binders.len(),
                            },
                        ));
                    }

                    let scope = self.scope();
                    for (binder, field) in arm.binders.iter().zip(fields) {
                        self.bind(binder.clone(), *field);
                    }
                    let arm_ty = match result_ty {
                        Some(ty) => self.check_expr(arm.body, ty).map(|()| ty),
                        None => self.infer(arm.body),
                    };
                    self.restore(scope);
                    result_ty = Some(arm_ty?);
                }

                let missing: Vec<String> = variants
                    .iter()
                    .filter(|(name, _)| !seen.contains(name))
                    .map(|(name, _)| name.clone())
                    .collect();
                if !missing.is_empty() {
                    return Err(err(span, TypeErrorKind::MissingVariants(missing)));
                }

                result_ty.expect("the parser guarantees at least one arm")
            }
        };

        let ty = match goal {
            Some(goal) => self.reconcile(ty, goal, span)?,
            None => ty,
        };
        self.ast.annotate_node(expr, ty);
        Ok(ty)
    }

    fn visit_lam(
        &mut self,
        expr: NodeId,
        span: Span,
        params: &[String],
        body: NodeId,
        goal: Option<TypeId>,
    ) -> Result<TypeId, TypeError> {
        let target = match (self.annotation(expr), goal) {
            (Some(ann), _) => {
                self.check_type(ann)?;
                ann
            }
            (None, Some(goal)) => goal,
            (None, None) => return Err(err(span, TypeErrorKind::CannotInfer("lambda"))),
        };

        let (generics, fn_ty) = self.split_forall(target);
        let Type::Function(param_tys, ret) = self.ty(fn_ty) else {
            return Err(err(span, TypeErrorKind::NotAFunction(target)));
        };

        let scope = self.scope();
        self.ty_vars.extend(generics);
        let result = self.lam_body(span, params, &param_tys, body, Some(ret));
        self.restore(scope);
        result?;

        Ok(target)
    }

    fn lam_body(
        &mut self,
        span: Span,
        params: &[String],
        param_tys: &[TypeId],
        body: NodeId,
        ret: Option<TypeId>,
    ) -> Result<TypeId, TypeError> {
        if params.len() != param_tys.len() {
            return Err(err(
                span,
                TypeErrorKind::ArityMismatch {
                    expected: param_tys.len(),
                    got: params.len(),
                },
            ));
        }

        let scope = self.scope();
        for (param, ty) in params.iter().zip(param_tys) {
            self.bind(param.clone(), *ty);
        }
        let result = match ret {
            Some(ret) => self.check_expr(body, ret).map(|()| ret),
            None => self.infer(body),
        };
        self.restore(scope);
        result
    }

    /// `callee(args)`. If the callee is generic its type arguments are solved as we go:
    ///
    /// 1. seed from the goal, by matching the return type against it;
    /// 2. process the arguments that can stand on their own, in order. Where the
    ///    parameter type is already fully known the argument is *checked*, otherwise
    ///    it is inferred and matched against the parameter type;
    /// 3. only then process the arguments that need a goal (un-annotated lambdas and
    ///    bare generic values such as `Nil`), now that most variables are known.
    fn visit_call(
        &mut self,
        span: Span,
        callee: NodeId,
        args: &[NodeId],
        goal: Option<TypeId>,
    ) -> Result<TypeId, TypeError> {
        let callee_ty = self.infer(callee)?;
        let (vars, fn_ty) = self.split_forall(callee_ty);
        let Type::Function(params, ret) = self.ty(fn_ty) else {
            return Err(err(span, TypeErrorKind::NotAFunction(callee_ty)));
        };

        if args.len() != params.len() {
            return Err(err(
                span,
                TypeErrorKind::ArityMismatch {
                    expected: params.len(),
                    got: args.len(),
                },
            ));
        }

        let mut solved = Solved::new();
        if let Some(goal) = goal {
            let mut seed = Solved::new();
            if self.match_type(ret, goal, &vars, &mut seed) {
                solved = seed;
            }
        }

        let (late, early): (Vec<usize>, Vec<usize>) =
            (0..args.len()).partition(|&i| self.needs_goal(args[i]));

        for i in early.into_iter().chain(late) {
            let (arg, pattern) = (args[i], params[i]);

            if self.unsolved(pattern, &vars, &solved).is_empty() {
                let want = self.substitute(pattern, &solved);
                self.check_expr(arg, want)?;
            } else if self.needs_goal(arg) {
                self.infer_late_arg(arg, pattern, &vars, &mut solved)?;
            } else {
                let got = self.infer(arg)?;
                if !self.match_type(pattern, got, &vars, &mut solved) {
                    let expected = self.substitute(pattern, &solved);
                    return Err(err(
                        self.node_span(arg),
                        TypeErrorKind::TypeMismatch { expected, got },
                    ));
                }
            }
        }

        let missing: Vec<String> = vars
            .iter()
            .filter(|v| !solved.contains_key(*v))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(err(span, TypeErrorKind::CannotInferTypeArgs(missing)));
        }

        // Record the instantiated function type on the callee itself.
        let instantiated = self.substitute(fn_ty, &solved);
        self.ast.annotate_node(callee, instantiated);

        Ok(self.substitute(ret, &solved))
    }

    /// An argument that needs a goal, but whose parameter type still mentions unsolved
    /// variables. The one case we can still handle is an un-annotated lambda whose
    /// *parameter* types are known: its body is inferred and that solves the result,
    /// e.g. the `B` in `map(xs, fun (x) => x + 1)`.
    fn infer_late_arg(
        &mut self,
        arg: NodeId,
        pattern: TypeId,
        vars: &[String],
        solved: &mut Solved,
    ) -> Result<(), TypeError> {
        let span = self.node_span(arg);

        let Node::Lam(params, body) = self.node(arg) else {
            let holes = self.unsolved(pattern, vars, solved);
            return Err(err(span, TypeErrorKind::CannotInferTypeArgs(holes)));
        };
        let Type::Function(pattern_params, pattern_ret) = self.ty(pattern) else {
            return Err(err(span, TypeErrorKind::NotAFunction(pattern)));
        };

        if pattern_params
            .iter()
            .any(|p| !self.unsolved(*p, vars, solved).is_empty())
        {
            return Err(err(
                span,
                TypeErrorKind::CannotInfer("lambda parameter type"),
            ));
        }
        let param_tys: Vec<TypeId> = pattern_params
            .iter()
            .map(|p| self.substitute(*p, solved))
            .collect();

        let body_ty = self.lam_body(span, &params, &param_tys, body, None)?;
        if !self.match_type(pattern_ret, body_ty, vars, solved) {
            let expected = self.substitute(pattern_ret, solved);
            return Err(err(
                self.node_span(body),
                TypeErrorKind::TypeMismatch {
                    expected,
                    got: body_ty,
                },
            ));
        }

        let lam_ty = self.substitute(pattern, solved);
        self.ast.annotate_node(arg, lam_ty);
        Ok(())
    }

    fn needs_goal(&self, arg: NodeId) -> bool {
        match self.ast.get_node(arg) {
            Node::Lam(..) => self.annotation(arg).is_none(),
            Node::Var(name) => self
                .lookup(name)
                .is_some_and(|ty| matches!(self.ast.get_type(ty), Type::Forall(..))),
            _ => false,
        }
    }

    fn reconcile(&mut self, got: TypeId, goal: TypeId, span: Span) -> Result<TypeId, TypeError> {
        let got = self.instantiate_against(got, goal);
        if self.converts(got, goal) {
            Ok(goal)
        } else {
            Err(err(
                span,
                TypeErrorKind::TypeMismatch {
                    expected: goal,
                    got,
                },
            ))
        }
    }

    fn instantiate_against(&mut self, got: TypeId, goal: TypeId) -> TypeId {
        let Type::Forall(vars, body) = self.ty(got) else {
            return got;
        };
        if matches!(self.ty(goal), Type::Forall(..)) {
            return got;
        }

        let mut solved = Solved::new();
        if self.match_type(body, goal, &vars, &mut solved)
            && vars.iter().all(|v| solved.contains_key(v))
        {
            self.substitute(body, &solved)
        } else {
            got
        }
    }

    /// One-way matching: find a binding for `vars` that makes `pattern` equal to
    /// `target`. Only `pattern` may contain variables from `vars`; a same-named
    /// variable in `target` is just an opaque rigid variable.
    fn match_type(
        &self,
        pattern: TypeId,
        target: TypeId,
        vars: &[String],
        solved: &mut Solved,
    ) -> bool {
        match (self.ty(pattern), self.ty(target)) {
            (Type::Named(n), _) if vars.contains(&n) => match solved.get(&n) {
                Some(&bound) => self.converts(bound, target),
                None => {
                    solved.insert(n, target);
                    true
                }
            },
            (Type::Named(a), Type::Named(b)) => a == b,
            (Type::Applied(a, xs), Type::Applied(b, ys)) => {
                a == b && self.match_all(&xs, &ys, vars, solved)
            }
            (Type::Function(ps, r), Type::Function(qs, s)) => {
                self.match_all(&ps, &qs, vars, solved) && self.match_type(r, s, vars, solved)
            }
            _ => false,
        }
    }

    fn match_all(
        &self,
        xs: &[TypeId],
        ys: &[TypeId],
        vars: &[String],
        solved: &mut Solved,
    ) -> bool {
        xs.len() == ys.len()
            && xs
                .iter()
                .zip(ys)
                .all(|(x, y)| self.match_type(*x, *y, vars, solved))
    }

    fn substitute(&mut self, ty: TypeId, map: &Solved) -> TypeId {
        if map.is_empty() {
            return ty;
        }

        let span = *self.ast.get_type_span(ty);
        match self.ty(ty) {
            Type::Named(n) => map.get(&n).copied().unwrap_or(ty),
            Type::Applied(n, args) => {
                let args = self.substitute_all(&args, map);
                self.ast.add_type(Type::Applied(n, args), span)
            }
            Type::Function(ps, r) => {
                let ps = self.substitute_all(&ps, map);
                let r = self.substitute(r, map);
                self.ast.add_type(Type::Function(ps, r), span)
            }
            Type::Forall(vs, body) => {
                let mut inner = map.clone();
                for v in &vs {
                    inner.remove(v);
                }
                let body = self.substitute(body, &inner);
                self.ast.add_type(Type::Forall(vs, body), span)
            }
        }
    }

    fn substitute_all(&mut self, tys: &[TypeId], map: &Solved) -> Vec<TypeId> {
        tys.iter().map(|t| self.substitute(*t, map)).collect()
    }

    fn unsolved(&self, ty: TypeId, vars: &[String], solved: &Solved) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_unsolved(ty, vars, solved, &mut out);
        out
    }

    fn collect_unsolved(
        &self,
        ty: TypeId,
        vars: &[String],
        solved: &Solved,
        out: &mut Vec<String>,
    ) {
        match self.ty(ty) {
            Type::Named(n) => {
                if vars.contains(&n) && !solved.contains_key(&n) && !out.contains(&n) {
                    out.push(n);
                }
            }
            Type::Applied(_, args) => {
                for a in args {
                    self.collect_unsolved(a, vars, solved, out);
                }
            }
            Type::Function(ps, r) => {
                for p in ps {
                    self.collect_unsolved(p, vars, solved, out);
                }
                self.collect_unsolved(r, vars, solved, out);
            }
            Type::Forall(_, body) => self.collect_unsolved(body, vars, solved, out),
        }
    }

    fn converts(&self, got: TypeId, expected: TypeId) -> bool {
        got == expected || self.equal(got, expected, &mut Vec::new())
    }

    fn equal(&self, a: TypeId, b: TypeId, bound: &mut Vec<(String, String)>) -> bool {
        match (self.ty(a), self.ty(b)) {
            (Type::Named(x), Type::Named(y)) => {
                for (l, r) in bound.iter().rev() {
                    if *l == x || *r == y {
                        return *l == x && *r == y;
                    }
                }
                x == y
            }
            (Type::Applied(x, xs), Type::Applied(y, ys)) => {
                x == y && self.equal_all(&xs, &ys, bound)
            }
            (Type::Function(ps, r), Type::Function(qs, s)) => {
                self.equal_all(&ps, &qs, bound) && self.equal(r, s, bound)
            }
            (Type::Forall(vs, x), Type::Forall(ws, y)) => {
                if vs.len() != ws.len() {
                    return false;
                }
                let depth = bound.len();
                bound.extend(vs.into_iter().zip(ws));
                let ok = self.equal(x, y, bound);
                bound.truncate(depth);
                ok
            }
            _ => false,
        }
    }

    fn equal_all(&self, xs: &[TypeId], ys: &[TypeId], bound: &mut Vec<(String, String)>) -> bool {
        xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| self.equal(*x, *y, bound))
    }

    fn check_type(&mut self, ty: TypeId) -> Result<(), TypeError> {
        let span = *self.ast.get_type_span(ty);
        match self.ty(ty) {
            Type::Named(n) => {
                if n == "num" || n == "bool" || self.ty_vars.contains(&n) {
                    return Ok(());
                }
                match self.adts.get(&n) {
                    Some(adt) if adt.vars.is_empty() => Ok(()),
                    Some(adt) => Err(err(
                        span,
                        TypeErrorKind::WrongTypeArity {
                            name: n,
                            expected: adt.vars.len(),
                            got: 0,
                        },
                    )),
                    None => Err(err(span, TypeErrorKind::UnknownType(n))),
                }
            }
            Type::Applied(n, args) => {
                let Some(adt) = self.adts.get(&n) else {
                    return Err(err(span, TypeErrorKind::UnknownType(n)));
                };
                if adt.vars.len() != args.len() {
                    return Err(err(
                        span,
                        TypeErrorKind::WrongTypeArity {
                            name: n,
                            expected: adt.vars.len(),
                            got: args.len(),
                        },
                    ));
                }
                args.into_iter().try_for_each(|a| self.check_type(a))
            }
            Type::Function(ps, r) => {
                ps.into_iter().try_for_each(|p| self.check_type(p))?;
                self.check_type(r)
            }
            Type::Forall(vars, body) => {
                let scope = self.scope();
                self.ty_vars.extend(vars);
                let result = self.check_type(body);
                self.restore(scope);
                result
            }
        }
    }

    fn split_forall(&self, ty: TypeId) -> (Vec<String>, TypeId) {
        match self.ty(ty) {
            Type::Forall(vars, inner) => (vars, inner),
            _ => (Vec::new(), ty),
        }
    }

    fn variants_of(&mut self, scrutinee: TypeId) -> Option<Vec<(String, Vec<TypeId>)>> {
        let (name, args) = match self.ty(scrutinee) {
            Type::Named(n) => (n, Vec::new()),
            Type::Applied(n, args) => (n, args),
            _ => return None,
        };
        let adt = self.adts.get(&name)?.clone();
        if adt.vars.len() != args.len() {
            return None;
        }

        let map: Solved = adt.vars.iter().cloned().zip(args).collect();
        let mut variants = Vec::new();
        for (ctor, fields) in &adt.ctors {
            let fields = self.substitute_all(fields, &map);
            variants.push((ctor.clone(), fields));
        }
        Some(variants)
    }

    fn ty_comparable(&self, ty: TypeId) -> bool {
        self.determine_comparable(ty, &[], &mut Vec::new())
    }

    fn determine_comparable(
        &self,
        ty: TypeId,
        assumed: &[String],
        visiting: &mut Vec<String>,
    ) -> bool {
        // assumed are types we are assuming to be true, specifically type vars
        match self.ty(ty) {
            Type::Named(n) if assumed.contains(&n) => true,
            Type::Named(n) => {
                n == "num" || n == "bool" || self.determine_adt_comparable(&n, visiting)
            }
            Type::Applied(b, vars) => {
                vars.iter()
                    .all(|a| self.determine_comparable(*a, assumed, visiting))
                    && self.determine_adt_comparable(&b, visiting)
            }
            Type::Function(..) | Type::Forall(..) => false,
        }
    }

    fn determine_adt_comparable(&self, name: &str, visiting: &mut Vec<String>) -> bool {
        if visiting.iter().any(|n| n == name) {
            return true;
        }

        let Some(adt) = self.adts.get(name) else {
            return false;
        };

        visiting.push(name.to_string());
        let ok = adt.ctors.iter().all(|(_, fields)| {
            fields
                .iter()
                .all(|f| self.determine_comparable(*f, &adt.vars, visiting))
        });
        visiting.pop();
        ok
    }

    fn prim(&mut self, name: &str, span: Span) -> TypeId {
        self.ast.add_type(Type::Named(name.to_string()), span)
    }

    fn type_app(&mut self, name: &str, vars: &[String], span: Span) -> TypeId {
        let type_node = if vars.is_empty() {
            Type::Named(name.to_string())
        } else {
            let args = vars
                .iter()
                .map(|v| {
                    let var_node = Type::Named(v.clone());
                    self.ast.add_type(var_node, span)
                })
                .collect();
            Type::Applied(name.to_string(), args)
        };
        self.ast.add_type(type_node, span)
    }

    fn forall(&mut self, vars: &[String], ty: TypeId, span: Span) -> TypeId {
        if vars.is_empty() {
            return ty;
        }

        let type_node = Type::Forall(vars.to_vec(), ty);
        self.ast.add_type(type_node, span)
    }

    fn match_record(
        &mut self,
        scrutinee: &str,
        ctor: &str,
        binders: &[String],
        body: NodeId,
        span: Span,
    ) -> NodeId {
        let scrut_node = Node::Var(scrutinee.to_string());
        let scut = self.ast.add_node(scrut_node, span);

        let arm = Arm {
            variant: ctor.to_string(),
            binders: binders.to_vec(),
            body,
        };

        let match_node = Node::Match(scut, vec![arm]);
        self.ast.add_node(match_node, span)
    }

    fn bind(&mut self, name: String, ty: TypeId) {
        self.env.push((name, ty));
    }

    fn scope(&self) -> Scope {
        Scope {
            env: self.env.len(),
            ty_vars: self.ty_vars.len(),
        }
    }

    fn restore(&mut self, scope: Scope) {
        self.env.truncate(scope.env);
        self.ty_vars.truncate(scope.ty_vars);
    }

    fn lookup(&self, name: &str) -> Option<TypeId> {
        self.env
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, t)| *t)
    }

    // The AST is mutated while we work, so these hand out owned values.
    fn node(&self, id: NodeId) -> Node {
        self.ast.get_node(id).clone()
    }

    fn ty(&self, id: TypeId) -> Type {
        self.ast.get_type(id).clone()
    }

    fn node_span(&self, id: NodeId) -> Span {
        *self.ast.get_node_span(id)
    }

    /// Before a node is visited this is only ever a user annotation.
    fn annotation(&self, id: NodeId) -> Option<TypeId> {
        self.ast.get_node_type(id)
    }
}

fn err(span: Span, kind: TypeErrorKind) -> TypeError {
    TypeError { span, kind }
}

#[derive(Debug)]
pub struct TypeError {
    pub span: Span,
    pub kind: TypeErrorKind,
}

/// Types are `TypeId`s into the program's arena, so use `TypeError::render` (which
/// needs the `Ast`) to print them.
#[derive(Debug)]
pub enum TypeErrorKind {
    TypeMismatch {
        expected: TypeId,
        got: TypeId,
    },
    UnboundVariable(String),
    NotAFunction(TypeId),
    ArityMismatch {
        expected: usize,
        got: usize,
    },
    CannotInfer(&'static str),
    CannotInferTypeArgs(Vec<String>),
    NotMatchable(TypeId),
    UnknownVariant {
        variant: String,
        ty: TypeId,
    },
    DuplicateArm(String),
    MissingVariants(Vec<String>),
    UnknownType(String),
    WrongTypeArity {
        name: String,
        expected: usize,
        got: usize,
    },
    DuplicateDefinition(String),
    NotComparable(TypeId),
}

impl TypeError {
    pub fn message(&self, ast: &Ast) -> String {
        use TypeErrorKind::*;
        let t = |id: TypeId| show_type(ast, id);

        match &self.kind {
            TypeMismatch { expected, got } => {
                format!("type mismatch: expected {}, got {}", t(*expected), t(*got))
            }
            UnboundVariable(name) => format!("unbound variable `{name}`"),
            NotAFunction(ty) => format!("expected a function, but this has type {}", t(*ty)),
            ArityMismatch { expected, got } => {
                format!("wrong number of arguments: expected {expected}, got {got}")
            }
            CannotInfer(what) => format!("cannot infer the type of this {what}; add an annotation"),
            CannotInferTypeArgs(vars) => {
                format!("cannot infer type argument(s): {}", vars.join(", "))
            }
            NotMatchable(ty) => format!("cannot match on a value of type {}", t(*ty)),
            UnknownVariant { variant, ty } => format!("`{variant}` is not a variant of {}", t(*ty)),
            DuplicateArm(variant) => format!("duplicate arm for `{variant}`"),
            MissingVariants(vs) => format!("match is missing variant(s): {}", vs.join(", ")),
            UnknownType(name) => format!("unknown type `{name}`"),
            WrongTypeArity {
                name,
                expected,
                got,
            } => {
                format!("type `{name}` takes {expected} argument(s), but {got} were given")
            }
            DuplicateDefinition(name) => format!("`{name}` is defined more than once"),
            NotComparable(ty) => format!("cannot compare values of type {}", t(*ty)),
        }
    }
}

pub fn show_type(ast: &Ast, ty: TypeId) -> String {
    let list = |tys: &[TypeId]| {
        tys.iter()
            .map(|t| show_type(ast, *t))
            .collect::<Vec<_>>()
            .join(", ")
    };

    match ast.get_type(ty) {
        Type::Named(n) => n.clone(),
        Type::Applied(n, args) => format!("{n}({})", list(args)),
        Type::Function(params, ret) => {
            let params = match params.as_slice() {
                [only] if !matches!(ast.get_type(*only), Type::Function(..) | Type::Forall(..)) => {
                    show_type(ast, *only)
                }
                _ => format!("({})", list(params)),
            };
            format!("{params} -> {}", show_type(ast, *ret))
        }
        Type::Forall(vars, body) => {
            format!("forall {}. {}", vars.join(", "), show_type(ast, *body))
        }
    }
}
