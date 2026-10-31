use crate::lexer::{HOST_FILE, Lexer, Span, Token, TokenKind};
use core::fmt;
use std::cmp::Ordering;
use std::fmt::Write as _;

pub struct Parser {
    token_source: Lexer,
    saved_tokens: Vec<Token>,
    position: usize,
    pub tree: Ast,
    furthest: Furthest,
}

pub struct Program {
    pub defs: Vec<Definition>,
    pub ast: Ast,
}

pub enum Definition {
    Function(FnDef),
    Constant(ConstDef),
    Type(TypeDef),
}

pub struct FnDef {
    pub span: Span,
    pub name: String,
    pub params: Vec<String>,
    pub typ: TypeId,
    pub body: NodeId,
}

pub struct ConstDef {
    pub span: Span,
    pub name: String,
    pub typ: TypeId,
    pub body: NodeId,
}

pub struct TypeDef {
    pub span: Span,
    pub name: String,
    pub vars: Vec<String>,
    pub body: TypeDefBody,
}

pub enum TypeDefBody {
    ADT(Vec<Variant>),
    Record(Vec<(String, TypeId)>),
}

pub struct Variant {
    pub name: String,
    pub args: Vec<TypeId>,
}

impl Parser {
    pub fn new(tokens: Lexer) -> Parser {
        Parser {
            token_source: tokens,
            saved_tokens: Vec::new(),
            position: 0,
            tree: Ast::new(),
            furthest: Furthest::default(),
        }
    }

    pub fn parse(&mut self) -> ParseResult<Vec<Definition>> {
        let mut defs = Vec::new();

        while let Ok(def) = self.parse_definition().ok_or_else(|| self.error()) {
            defs.push(def);
        }

        self.expect(TokenKind::End).ok_or_else(|| self.error())?;
        Ok(defs)
    }

    fn mark(&self) -> Mark {
        Mark(
            self.position,
            self.tree.nodes.len(),
            self.tree.types.len(),
            self.tree.node_types.len(),
        )
    }

    fn reset(&mut self, mark: Mark) {
        self.position = mark.0;
        self.tree.nodes.truncate(mark.1);
        self.tree.node_spans.truncate(mark.1);
        self.tree.types.truncate(mark.2);
        self.tree.type_spans.truncate(mark.2);
        self.tree.node_types.truncate(mark.3);
    }

    fn attempt<T>(&mut self, f: impl FnOnce(&mut Self) -> Option<T>) -> Option<T> {
        let position = self.mark();

        let result = f(self);
        if result.is_none() {
            self.reset(position);
        }

        result
    }

    fn current(&mut self) -> Token {
        while self.position >= self.saved_tokens.len() {
            let new_token = self.token_source.lex_token();
            self.saved_tokens.push(new_token);
        }

        self.saved_tokens[self.position]
    }

    fn expect(&mut self, kind: TokenKind) -> Option<Token> {
        let token = self.current();
        if token.kind == kind {
            self.position += 1;
            return Some(token);
        }

        self.note_expected(kind);
        None
    }

    fn note_expected(&mut self, kind: TokenKind) {
        let furthest = &mut self.furthest;
        match self.position.cmp(&furthest.position) {
            Ordering::Greater => {
                furthest.position = self.position;
                furthest.expected.clear();
                furthest.expected.push(kind);
            }
            Ordering::Equal if !furthest.expected.contains(&kind) => {
                furthest.expected.push(kind);
            }
            _ => {}
        }
    }

    fn error(&self) -> ParseError {
        let found = self.saved_tokens[self.furthest.position];
        ParseError {
            span: found.span,
            found: found.kind,
            expected: self.furthest.expected.clone(),
        }
    }

    fn peek_is(&mut self, kind: TokenKind) -> Option<Token> {
        let token = self.current();
        if token.kind == kind {
            return Some(token);
        }

        None
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        self.expect(kind).is_some()
    }

    fn parse_definition(&mut self) -> Option<Definition> {
        // DEF ::= VALUE_DEF | TYPE_DEF
        self.parse_value_definition()
            .or_else(|| self.parse_type_definition())
    }

    fn parse_value_definition(&mut self) -> Option<Definition> {
        // VALUE_DEF ::= "def" IDENT ANNOTATED_PARAMS "=" EXPR ";"
        self.attempt(|p| {
            let start = p.expect(TokenKind::Def)?;
            let name = p.expect(TokenKind::Identifier)?;
            let (params, typ) = if p.peek_is(TokenKind::LParen).is_some() {
                p.parse_annotated_params()?
            } else {
                p.note_expected(TokenKind::LParen);

                p.expect(TokenKind::Colon)?;
                let typ = p.parse_type()?;
                (Vec::new(), typ)
            };
            p.expect(TokenKind::Eq)?;
            let body = p.parse_expr()?;
            let end = p.expect(TokenKind::Semi)?;

            let span = start.span.merge_with(end.span);
            let name = p.string_from_token(name);

            Some(if params.is_empty() {
                Definition::Constant(ConstDef {
                    span,
                    name,
                    typ,
                    body,
                })
            } else {
                Definition::Function(FnDef {
                    span,
                    name,
                    params,
                    typ,
                    body,
                })
            })
        })
    }

    fn parse_type_definition(&mut self) -> Option<Definition> {
        // TYPE_DEF ::= "type" IDENT ["(" IDENT ("," IDENT)* ")"] "=" TYPE_DEF_BODY ";"
        self.attempt(|p| {
            let start = p.expect(TokenKind::Type)?;
            let name = p.expect(TokenKind::Identifier)?;

            let vars = if p.eat(TokenKind::LParen) {
                let vars = p.parse_comma_separated(|p| {
                    let var = p.expect(TokenKind::Identifier)?;
                    Some(p.string_from_token(var))
                });
                p.expect(TokenKind::RParen)?;
                vars
            } else {
                Vec::new()
            };

            p.expect(TokenKind::Eq)?;

            let body = p.parse_type_definition_body()?;

            let end = p.expect(TokenKind::Semi)?;

            let span = start.span.merge_with(end.span);
            let name = p.string_from_token(name);

            Some(Definition::Type(TypeDef {
                span,
                name,
                vars,
                body,
            }))
        })
    }

    fn parse_type_definition_body(&mut self) -> Option<TypeDefBody> {
        // TYPE_DEF_BODY ::= "record" "{" PARAMS "}" | VARIANT+
        if self.eat(TokenKind::Record) {
            self.attempt(|p| {
                p.expect(TokenKind::LBrace)?;

                let fields = p.parse_comma_separated(|p| {
                    let name = p.expect(TokenKind::Identifier)?;
                    let name = p.string_from_token(name);
                    p.expect(TokenKind::Colon)?;
                    let typ = p.parse_type()?;
                    Some((name, typ))
                });

                p.expect(TokenKind::RBrace)?;
                Some(TypeDefBody::Record(fields))
            })
        } else {
            let mut variants = Vec::new();

            while let Some(variant) = self.parse_variant() {
                variants.push(variant);
            }

            if variants.is_empty() {
                return None;
            }

            Some(TypeDefBody::ADT(variants))
        }
    }

    fn parse_variant(&mut self) -> Option<Variant> {
        // VARIANT ::= "|" IDENT [ "(" TYPE ("," TYPE)* ")" ]
        self.attempt(|p| {
            p.expect(TokenKind::Pipe)?;
            let name = p.expect(TokenKind::Identifier)?;

            let args = if p.eat(TokenKind::LParen) {
                let args = p.parse_comma_separated(Self::parse_type);
                p.expect(TokenKind::RParen)?;
                args
            } else {
                Vec::new()
            };

            let name = p.string_from_token(name);
            Some(Variant { name, args })
        })
    }

    fn parse_expr(&mut self) -> Option<NodeId> {
        // EXPR ::= LET | LAM | IF | MATCH | OR
        self.parse_let()
            .or_else(|| self.parse_lam())
            .or_else(|| self.parse_if())
            .or_else(|| self.parse_match())
            .or_else(|| self.parse_or())
    }

    fn parse_let(&mut self) -> Option<NodeId> {
        // LET ::= "let" IDENT (":=" | ":" TYPE "=") EXPR ";" EXPR
        self.attempt(|p| {
            let kw = p.expect(TokenKind::Let)?;
            let name = p.expect(TokenKind::Identifier)?;
            let ty = if p.eat(TokenKind::ColonEq) {
                None
            } else {
                p.note_expected(TokenKind::ColonEq);

                p.expect(TokenKind::Colon)?;
                let ty = p.parse_type()?;
                p.expect(TokenKind::Eq)?;
                Some(ty)
            };

            let value = p.parse_expr()?;
            if let Some(typ) = ty {
                p.annotate_node(value, typ);
            };

            p.expect(TokenKind::Semi)?;
            let body = p.parse_expr()?;

            let span = kw.span.merge_with(p.span_of_node(body));
            let name = p.string_from_token(name);
            let node = p.add_node(Node::Let(name, value, body), span);

            Some(node)
        })
    }

    fn parse_lam(&mut self) -> Option<NodeId> {
        // LAM ::= "fun" LAM_PARAMS "=>" EXPR
        self.attempt(|p| {
            let kw = p.expect(TokenKind::Fun)?;
            let (params, typ) = p.parse_lam_params()?;
            p.expect(TokenKind::BigArrow)?;
            let body = p.parse_expr()?;

            let span = kw.span.merge_with(p.span_of_node(body));
            let node = p.add_node(Node::Lam(params, body), span);

            if let Some(typ) = typ {
                p.annotate_node(node, typ);
            }

            Some(node)
        })
    }

    fn parse_lam_params(&mut self) -> Option<(Vec<String>, Option<TypeId>)> {
        // LAM_PARAMS ::= ANNOTATED_PARAMS | UNANNOTATED_PARAMS
        self.parse_annotated_params()
            .map(|(params, ty)| (params, Some(ty)))
            .or_else(|| self.parse_unannotated_params().map(|params| (params, None)))
    }

    fn parse_annotated_params(&mut self) -> Option<(Vec<String>, TypeId)> {
        // ANNOTATED_PARAMS ::= "(" ANNOTATED_PARAM ("," ANNOTATED_PARAM)* ")" ":" TYPE
        self.attempt(|p| {
            let start = p.expect(TokenKind::LParen)?;
            let params = p.parse_comma_separated(Self::parse_annotated_param);

            if params.is_empty() {
                return None;
            }

            p.expect(TokenKind::RParen)?;
            p.expect(TokenKind::Colon)?;
            let result = p.parse_type()?;

            let mut generics = Vec::new();
            let mut names = Vec::new();
            let mut types = Vec::new();

            for param in params {
                match param {
                    Param::Implicit(n) => {
                        generics.push(n);
                    }
                    Param::Explicit(n, t) => {
                        names.push(n);
                        types.push(t);
                    }
                }
            }

            let span = start.span.merge_with(p.span_of_type(result));

            let mut typ = p.add_type(Type::Function(types, result), span);
            if !generics.is_empty() {
                typ = p.add_type(Type::Forall(generics, typ), span)
            }

            Some((names, typ))
        })
    }

    fn parse_annotated_param(&mut self) -> Option<Param> {
        // ANNOTATED_PARAM ::= "@" IDENT ":" "type" | IDENT ":" TYPE
        self.attempt(|p| {
            if p.eat(TokenKind::At) {
                let name = p.expect(TokenKind::Identifier)?;
                let name = p.string_from_token(name);
                p.expect(TokenKind::Colon)?;
                p.expect(TokenKind::Type)?;
                Some(Param::Implicit(name))
            } else {
                let name = p.expect(TokenKind::Identifier)?;
                let name = p.string_from_token(name);
                p.expect(TokenKind::Colon)?;
                let typ = p.parse_type()?;
                Some(Param::Explicit(name, typ))
            }
        })
    }

    fn parse_unannotated_params(&mut self) -> Option<Vec<String>> {
        // UNANNOTATED_PARAMS ::= "(" IDENT ("," IDENT)* ")"
        self.attempt(|p| {
            p.expect(TokenKind::LParen)?;
            let params = p.parse_comma_separated(|p| {
                let name = p.expect(TokenKind::Identifier)?;
                Some(p.string_from_token(name))
            });

            if params.is_empty() {
                return None;
            }

            p.expect(TokenKind::RParen)?;
            Some(params)
        })
    }

    fn parse_if(&mut self) -> Option<NodeId> {
        // IF ::= "if" EXPR "then" EXPR "else" EXPR
        self.attempt(|p| {
            let kw = p.expect(TokenKind::If)?;
            let cond = p.parse_expr()?;
            p.expect(TokenKind::Then)?;
            let then = p.parse_expr()?;
            p.expect(TokenKind::Else)?;
            let els = p.parse_expr()?;

            let span = kw.span.merge_with(p.span_of_node(els));
            Some(p.add_node(Node::If(cond, then, els), span))
        })
    }

    fn parse_match(&mut self) -> Option<NodeId> {
        // MATCH ::= "match" EXPR NORMAL_ARMS* LAST_ARM
        self.attempt(|p| {
            let start = p.expect(TokenKind::Match)?;
            let scrutinee = p.parse_expr()?;

            let mut arms = Vec::new();
            while let Some(arm) = p.attempt(Self::parse_normal_arm) {
                arms.push(arm);
            }

            let last_arm = p.parse_last_arm()?;
            let end = p.span_of_node(last_arm.body);
            arms.push(last_arm);

            let node = Node::Match(scrutinee, arms);
            let span = start.span.merge_with(end);
            Some(p.add_node(node, span))
        })
    }

    fn parse_normal_arm(&mut self) -> Option<Arm> {
        // NORMAL_ARM ::= "|" IDENT ["(" IDENT ("," IDENT)* ")"] "=>" EXPR
        self.attempt(|p| {
            p.expect(TokenKind::Pipe)?;
            let variant = p.expect(TokenKind::Identifier)?;
            let variant = p.string_from_token(variant);

            let binders = if p.eat(TokenKind::LParen) {
                let binders = p.parse_comma_separated(|p| {
                    let token = p.expect(TokenKind::Identifier)?;
                    Some(p.string_from_token(token))
                });
                p.expect(TokenKind::RParen)?;
                binders
            } else {
                Vec::new()
            };

            p.expect(TokenKind::BigArrow)?;
            let body = p.parse_expr()?;

            Some(Arm {
                variant,
                binders,
                body,
            })
        })
    }

    fn parse_last_arm(&mut self) -> Option<Arm> {
        // LAST_ARM ::= "\" IDENT ["(" IDENT ("," IDENT)* ")"] "=>" EXPR
        self.attempt(|p| {
            p.expect(TokenKind::BSlash)?;
            let variant = p.expect(TokenKind::Identifier)?;
            let variant = p.string_from_token(variant);

            let binders = if p.eat(TokenKind::LParen) {
                let binders = p.parse_comma_separated(|p| {
                    let token = p.expect(TokenKind::Identifier)?;
                    Some(p.string_from_token(token))
                });
                p.expect(TokenKind::RParen)?;
                binders
            } else {
                Vec::new()
            };

            p.expect(TokenKind::BigArrow)?;
            let body = p.parse_expr()?;

            Some(Arm {
                variant,
                binders,
                body,
            })
        })
    }

    fn parse_or(&mut self) -> Option<NodeId> {
        // OR ::= AND ("or" AND)*
        self.parse_left_assoc(Self::parse_and, &[(TokenKind::Or, Node::Or)])
    }

    fn parse_and(&mut self) -> Option<NodeId> {
        // AND ::= NOT ("and" NOT)*
        self.parse_left_assoc(Self::parse_not, &[(TokenKind::And, Node::And)])
    }

    fn parse_not(&mut self) -> Option<NodeId> {
        // NOT ::= "not" NOT | COMP
        self.attempt(|p| {
            let kw = p.expect(TokenKind::Not)?;
            let operand = p.parse_not()?;

            let span = kw.span.merge_with(p.span_of_node(operand));
            Some(p.add_node(Node::Not(operand), span))
        })
        .or_else(|| self.parse_comparison())
    }

    fn parse_comparison(&mut self) -> Option<NodeId> {
        // COMP ::= TERM ("<" | "<=" | ">" | ">=" | "==" | "/=" TERM)*
        self.parse_left_assoc(
            Self::parse_term,
            &[
                (TokenKind::Less, Node::Lt),
                (TokenKind::LessEq, Node::Le),
                (TokenKind::Greater, Node::Gt),
                (TokenKind::GreaterEq, Node::Ge),
                (TokenKind::EqEq, Node::Eq),
                (TokenKind::Neq, Node::Ne),
            ],
        )
    }

    fn parse_term(&mut self) -> Option<NodeId> {
        // TERM ::= FACTOR ("+" | "-" FACTOR)*
        self.parse_left_assoc(
            Self::parse_factor,
            &[(TokenKind::Plus, Node::Add), (TokenKind::Dash, Node::Sub)],
        )
    }

    fn parse_factor(&mut self) -> Option<NodeId> {
        // FACTOR ::= POSTFIX ("*" | "/" POSTFIX)*
        self.parse_left_assoc(
            Self::parse_prefix,
            &[(TokenKind::Star, Node::Mul), (TokenKind::Slash, Node::Div)],
        )
    }

    fn parse_prefix(&mut self) -> Option<NodeId> {
        // PREFIX ::= "-" PREFIX | POSTFIX
        self.attempt(|p| {
            let start = p.expect(TokenKind::Dash)?;
            let operand = p.parse_prefix()?;

            let span = start.span.merge_with(p.span_of_node(operand));
            Some(p.add_node(Node::Neg(operand), span))
        })
        .or_else(|| self.parse_postfix())
    }

    fn parse_postfix(&mut self) -> Option<NodeId> {
        // POSTFIX ::= SUFFIXES | SECTION
        match self.parse_primary() {
            Some(primary) => Some(self.parse_suffixes(primary)),
            None => self.parse_section(),
        }
    }

    fn parse_suffixes(&mut self, mut base_node: NodeId) -> NodeId {
        // SUFFIXES ::= PRIMARY SUFFIX*
        let start = self.span_of_node(base_node);
        while let Some(next_node) = self.parse_suffix(base_node, start) {
            base_node = next_node;
        }

        base_node
    }

    fn parse_suffix(&mut self, base_node: NodeId, start: Span) -> Option<NodeId> {
        // SUFFIX ::= CALL | DOT
        self.parse_call(base_node, start)
            .or_else(|| self.parse_dot(base_node, start))
    }

    fn parse_call(&mut self, base_node: NodeId, start: Span) -> Option<NodeId> {
        // CALL ::= "(" ARGS ")"
        self.attempt(|p| {
            p.expect(TokenKind::LParen)?;
            let args = p.parse_args();
            let end = p.expect(TokenKind::RParen)?;

            let span = start.merge_with(end.span);
            Some(p.add_node(Node::Call(base_node, args), span))
        })
    }

    fn parse_dot(&mut self, base_node: NodeId, start: Span) -> Option<NodeId> {
        // DOT ::= "." IDENT | "." IDENT "(" ARGS ")"
        self.attempt(|p| {
            p.expect(TokenKind::Dot)?;
            let name = p.expect(TokenKind::Identifier)?;

            let args_list = p.attempt(|p| {
                p.expect(TokenKind::LParen)?;
                let args = p.parse_args();
                let close = p.expect(TokenKind::RParen)?;
                Some((args, close.span))
            });

            let (mut args, end) = args_list.unwrap_or((Vec::new(), name.span));
            args.insert(0, base_node);

            let callee_node = Node::Var(p.string_from_token(name));
            let callee = p.add_node(callee_node, name.span);

            let span = start.merge_with(end);
            Some(p.add_node(Node::Call(callee, args), span))
        })
    }

    fn parse_section(&mut self) -> Option<NodeId> {
        // SECTION ::= "." IDENT SUFFIX*
        self.attempt(|p| {
            let dot = p.peek_is(TokenKind::Dot)?;
            let it = p.add_node(Node::Var("$it".to_string()), dot.span);
            let body = p.parse_suffixes(it);

            if body == it {
                return None;
            }

            let span = p.span_of_node(body);
            let params = vec!["$it".to_string()];
            Some(p.add_node(Node::Lam(params, body), span))
        })
    }

    fn parse_primary(&mut self) -> Option<NodeId> {
        // PRIMARY ::= NUMBER | BOOL | IDENT | "(" EXPR ")"
        if let Some(number) = self.expect(TokenKind::Number) {
            return Some(self.num_from_token(number));
        }

        if let Some(t) = self.expect(TokenKind::True) {
            return Some(self.add_node(Node::Bool(true), t.span));
        }

        if let Some(f) = self.expect(TokenKind::False) {
            return Some(self.add_node(Node::Bool(false), f.span));
        }

        if let Some(var) = self.expect(TokenKind::Identifier) {
            return Some(self.var_from_token(var));
        }

        self.attempt(|p| {
            p.expect(TokenKind::LParen)?;
            let expr = p.parse_expr()?;
            p.expect(TokenKind::RParen)?;
            Some(expr)
        })
    }

    fn parse_args(&mut self) -> Vec<NodeId> {
        // ARGS ::= EXPR ("," EXPR)* ","?
        self.parse_comma_separated(Self::parse_expr)
    }

    fn parse_type(&mut self) -> Option<TypeId> {
        // TYPE ::= FN_TYPE
        self.parse_fn_type()
    }

    fn parse_fn_type(&mut self) -> Option<TypeId> {
        // FN_TYPE ::= PARAM_TYPE ["->" TYPE]
        self.attempt(|p| {
            let (start, params) = p.parse_param_type()?;

            if p.eat(TokenKind::Arrow) {
                let result = p.parse_type()?;
                let fn_node = Type::Function(params, result);
                let fn_span = start.merge_with(p.span_of_type(result));
                return Some(p.add_type(fn_node, fn_span));
            }

            match params.as_slice() {
                &[single] => Some(single),
                _ => return None,
            }
        })
    }

    fn parse_param_type(&mut self) -> Option<(Span, Vec<TypeId>)> {
        // PARAM_TYPE ::= SIMPLE_TYPE | "(" TYPE ("," TYPE)+ ")"
        self.parse_simple_type()
            .map(|(start, typ)| (start, vec![typ]))
            .or_else(|| {
                self.attempt(|p| {
                    let start = p.expect(TokenKind::LParen)?;
                    let params = p.parse_comma_separated(Self::parse_type);
                    p.expect(TokenKind::RParen)?;

                    if params.len() < 2 {
                        return None;
                    }

                    Some((start.span, params))
                })
            })
    }

    fn parse_simple_type(&mut self) -> Option<(Span, TypeId)> {
        // SIMPLE_TYPE ::= IDENT ["(" TYPE ("," TYPE)* ")"] | "(" TYPE ")"
        self.attempt(|p| {
            let name_t = p.expect(TokenKind::Identifier)?;
            let name = p.string_from_token(name_t);

            let typ = if p.eat(TokenKind::LParen) {
                let args = p.parse_comma_separated(Self::parse_type);
                let end = p.expect(TokenKind::RParen)?;
                p.add_type(Type::Applied(name, args), name_t.span.merge_with(end.span))
            } else {
                p.add_type(Type::Named(name), name_t.span)
            };

            Some((name_t.span, typ))
        })
        .or_else(|| {
            self.attempt(|p| {
                let start = p.expect(TokenKind::LParen)?;
                let typ = p.parse_type()?;
                p.expect(TokenKind::RParen)?;
                Some((start.span, typ))
            })
        })
    }

    fn parse_left_assoc(
        &mut self,
        operand: fn(&mut Self) -> Option<NodeId>,
        ops: &[(TokenKind, fn(NodeId, NodeId) -> Node)],
    ) -> Option<NodeId> {
        let mut left = operand(self)?;

        loop {
            let next = self.attempt(|p| {
                let kind = p.current().kind;
                let make = ops.iter().find(|op| op.0 == kind)?.1;
                p.position += 1;

                let right = operand(p)?;
                let span = p.merge_spans(left, right);
                Some(p.add_node(make(left, right), span))
            });

            match next {
                Some(node) => left = node,
                None => return Some(left),
            }
        }
    }

    fn parse_comma_separated<T>(&mut self, mut item: impl FnMut(&mut Self) -> Option<T>) -> Vec<T> {
        let mut items = Vec::new();

        let Some(first) = item(self) else {
            return items;
        };

        items.push(first);
        while let Some(next) = self.attempt(|p| {
            p.expect(TokenKind::Comma)?;
            item(p)
        }) {
            items.push(next);
        }

        self.eat(TokenKind::Comma);
        items
    }

    fn add_node(&mut self, node: Node, span: Span) -> NodeId {
        self.tree.add_node(node, span)
    }

    fn add_type(&mut self, typ: Type, span: Span) -> TypeId {
        self.tree.add_type(typ, span)
    }

    fn annotate_node(&mut self, node: NodeId, typ: TypeId) {
        self.tree.annotate_node(node, typ);
    }

    fn float_from_token(&self, token: Token) -> f64 {
        self.token_source.token_content(token).parse().unwrap()
    }

    fn string_from_token(&self, token: Token) -> String {
        self.token_source.token_content(token).to_string()
    }

    fn num_from_token(&mut self, token: Token) -> NodeId {
        self.add_node(Node::Num(self.float_from_token(token)), token.span)
    }

    fn var_from_token(&mut self, token: Token) -> NodeId {
        self.add_node(Node::Var(self.string_from_token(token)), token.span)
    }

    fn _named_from_token(&mut self, token: Token) -> TypeId {
        self.add_type(Type::Named(self.string_from_token(token)), token.span)
    }

    fn span_of_node(&self, node: NodeId) -> Span {
        self.tree.node_spans[node.0]
    }

    fn span_of_type(&self, typ: TypeId) -> Span {
        self.tree.type_spans[typ.0]
    }

    fn merge_spans(&self, left: NodeId, right: NodeId) -> Span {
        self.span_of_node(left).merge_with(self.span_of_node(right))
    }
}

pub fn parse_type_into(ty: &str, ast: &mut Ast) -> ParseResult<TypeId> {
    let lexer = Lexer::new(ty.to_string(), HOST_FILE);
    let mut parser = Parser::new(lexer);

    parser.tree = std::mem::take(ast);

    let result = parser.attempt(|p| {
        let ty = p.parse_type()?;
        p.expect(TokenKind::End)?;
        Some(ty)
    });

    let result = result.ok_or_else(|| parser.error());

    *ast = parser.tree;
    result
}

pub fn parse_file_into(source: &str, file_id: u32, ast: &mut Ast) -> ParseResult<Vec<Definition>> {
    let lexer = Lexer::new(source.to_string(), file_id);
    let mut parser = Parser::new(lexer);

    parser.tree = std::mem::take(ast);
    let result = parser.parse();
    *ast = parser.tree;

    result
}

type ParseResult<T> = Result<T, ParseError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub span: Span,
    pub found: TokenKind,
    pub expected: Vec<TokenKind>,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.span, self.message())
    }
}

impl ParseError {
    pub fn message(&self) -> String {
        let mut out = format!("Unexpected {}", self.found);
        match self.expected.as_slice() {
            [] => out.push('.'),
            [only] => write!(out, ", expected {}.", only).unwrap(),
            [rest @ .., last] => {
                let rest: Vec<String> = rest.iter().map(|k| k.to_string()).collect();
                write!(out, ", expected one of {}, or {}.", rest.join(", "), last).unwrap();
            }
        }
        out
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mark(usize, usize, usize, usize);

#[derive(Debug, Default)]
struct Furthest {
    position: usize,
    expected: Vec<TokenKind>,
}

#[derive(Debug)]
pub struct Ast {
    nodes: Vec<Node>,                // Expression values
    types: Vec<Type>,                // Type values
    node_spans: Vec<Span>,           // Assosciated span
    type_spans: Vec<Span>,           // Assosciated span
    node_types: Vec<Option<TypeId>>, // Annotated or inferred type (keyed by NodeId)
}

impl Default for Ast {
    fn default() -> Self {
        Ast::new()
    }
}

impl Ast {
    pub fn new() -> Ast {
        Ast {
            nodes: Vec::new(),
            types: Vec::new(),
            node_spans: Vec::new(),
            type_spans: Vec::new(),
            node_types: Vec::new(),
        }
    }

    pub fn get_node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    pub fn get_type(&self, id: TypeId) -> &Type {
        &self.types[id.0]
    }

    pub fn get_node_span(&self, id: NodeId) -> &Span {
        &self.node_spans[id.0]
    }

    pub fn get_type_span(&self, id: TypeId) -> &Span {
        &self.type_spans[id.0]
    }

    pub fn get_node_type(&self, id: NodeId) -> Option<TypeId> {
        self.node_types[id.0]
    }

    pub fn add_node(&mut self, node: Node, span: Span) -> NodeId {
        let id = self.nodes.len();

        self.nodes.push(node);
        self.node_spans.push(span);
        self.node_types.push(None);

        NodeId(id)
    }

    pub fn add_type(&mut self, typ: Type, span: Span) -> TypeId {
        let id = self.types.len();

        self.types.push(typ);
        self.type_spans.push(span);

        TypeId(id)
    }

    pub fn annotate_node(&mut self, node: NodeId, ty: TypeId) {
        self.node_types[node.0] = Some(ty);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeId(usize);

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Num(f64),
    Bool(bool),
    Var(String),
    Add(NodeId, NodeId),
    Sub(NodeId, NodeId),
    Mul(NodeId, NodeId),
    Div(NodeId, NodeId),
    Lt(NodeId, NodeId),
    Le(NodeId, NodeId),
    Gt(NodeId, NodeId),
    Ge(NodeId, NodeId),
    Eq(NodeId, NodeId),
    Ne(NodeId, NodeId),
    Or(NodeId, NodeId),
    And(NodeId, NodeId),
    Not(NodeId),
    Neg(NodeId),
    Let(String, NodeId, NodeId),
    Lam(Vec<String>, NodeId),
    Call(NodeId, Vec<NodeId>),
    If(NodeId, NodeId, NodeId),
    Match(NodeId, Vec<Arm>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arm {
    pub variant: String,
    pub binders: Vec<String>,
    pub body: NodeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypeId(usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Named(String),
    Applied(String, Vec<TypeId>),
    Function(Vec<TypeId>, TypeId),
    Forall(Vec<String>, TypeId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Param {
    Implicit(String),
    Explicit(String, TypeId),
}
