use std::fmt;

pub struct Lexer {
    data: String,
    span: Span,
}

impl Lexer {
    pub fn new(source: String, file: u32) -> Lexer {
        Lexer {
            data: source,
            span: Span::new(file),
        }
    }

    pub fn lex_token(&mut self) -> Token {
        self.lex_whitespace();
        self.span = Span {
            file: self.span.file,
            start: self.span.end,
            end: self.span.end,
        };

        match self.advance() {
            Some(b'@') => self.make_token(TokenKind::At),
            Some(b'+') => self.make_token(TokenKind::Plus),
            Some(b'*') => self.make_token(TokenKind::Star),
            Some(b'(') => self.make_token(TokenKind::LParen),
            Some(b')') => self.make_token(TokenKind::RParen),
            Some(b'{') => self.make_token(TokenKind::LBrace),
            Some(b'}') => self.make_token(TokenKind::RBrace),
            Some(b';') => self.make_token(TokenKind::Semi),
            Some(b',') => self.make_token(TokenKind::Comma),
            Some(b'.') => self.make_token(TokenKind::Dot),
            Some(b'\\') => self.make_token(TokenKind::BSlash),
            Some(b'|') => self.make_token(TokenKind::Pipe),

            Some(b':') => self.consume_as_else(b'=', TokenKind::ColonEq, TokenKind::Colon),
            Some(b'-') => self.consume_as_else(b'>', TokenKind::Arrow, TokenKind::Dash),
            Some(b'<') => self.consume_as_else(b'=', TokenKind::LessEq, TokenKind::Less),
            Some(b'>') => self.consume_as_else(b'=', TokenKind::GreaterEq, TokenKind::Greater),
            Some(b'/') => self.consume_as_else(b'=', TokenKind::Neq, TokenKind::Slash),

            Some(b'=') => match self.peek() {
                Some(b'=') => self.consume_as(TokenKind::EqEq),
                Some(b'>') => self.consume_as(TokenKind::BigArrow),
                _ => self.make_token(TokenKind::Eq),
            },

            Some(b'0'..=b'9') => self.lex_number(),
            Some(b'_' | b'a'..=b'z' | b'A'..=b'Z') => self.lex_ident(),

            None => self.make_token(TokenKind::End),
            Some(_) => {
                self.align_on_char();
                self.make_token(TokenKind::Unknown)
            }
        }
    }

    pub fn token_content(&self, token: Token) -> &str {
        self.content_for_span(token.span)
    }

    fn content_for_span(&self, span: Span) -> &str {
        &self.data[span.start..span.end]
    }

    fn advance(&mut self) -> Option<u8> {
        let current = *self.data.as_bytes().get(self.span.end)?;
        self.span.end += 1;
        Some(current)
    }

    fn retreat(&mut self) {
        self.span.end -= 1;
    }

    fn align_on_char(&mut self) {
        while !self.data.is_char_boundary(self.span.end) {
            self.advance();
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.data.as_bytes().get(self.span.end).copied()
    }

    fn consume_as(&mut self, kind: TokenKind) -> Token {
        self.advance();
        self.make_token(kind)
    }

    fn consume_as_else(&mut self, byte: u8, then: TokenKind, els: TokenKind) -> Token {
        if self.matches(|ch| ch == byte) {
            return self.consume_as(then);
        }

        self.make_token(els)
    }

    fn matches<P>(&self, predicate: P) -> bool
    where
        P: Fn(u8) -> bool,
    {
        match self.data.as_bytes().get(self.span.end) {
            Some(&ch) => predicate(ch),
            _ => false,
        }
    }

    fn make_token(&self, kind: TokenKind) -> Token {
        Token {
            kind,
            span: self.span,
        }
    }

    fn lex_number(&mut self) -> Token {
        while self.matches(|ch| ch.is_ascii_digit()) {
            self.advance();
        }

        if self.matches(|ch| ch == b'.') {
            self.advance();

            if self.matches(|ch| ch.is_ascii_digit()) {
                while self.matches(|ch| ch.is_ascii_digit()) {
                    self.advance();
                }
            } else {
                self.retreat();
            }
        }

        self.make_token(TokenKind::Number)
    }

    fn lex_ident(&mut self) -> Token {
        while self.matches(|ch| ch == b'_' || ch.is_ascii_alphabetic() || ch.is_ascii_digit()) {
            self.advance();
        }

        match self.content_for_span(self.span) {
            "and" => self.make_token(TokenKind::And),
            "or" => self.make_token(TokenKind::Or),
            "not" => self.make_token(TokenKind::Not),
            "true" => self.make_token(TokenKind::True),
            "false" => self.make_token(TokenKind::False),
            "let" => self.make_token(TokenKind::Let),
            "fun" => self.make_token(TokenKind::Fun),
            "if" => self.make_token(TokenKind::If),
            "then" => self.make_token(TokenKind::Then),
            "else" => self.make_token(TokenKind::Else),
            "match" => self.make_token(TokenKind::Match),
            "def" => self.make_token(TokenKind::Def),
            "type" => self.make_token(TokenKind::Type),
            "record" => self.make_token(TokenKind::Record),
            _ => self.make_token(TokenKind::Identifier),
        }
    }

    fn lex_whitespace(&mut self) {
        while self.matches(|ch| ch.is_ascii_whitespace()) {
            self.advance();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Number,
    Identifier,
    At,
    Plus,
    Star,
    Dash,
    Slash,
    BSlash,
    Pipe,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    EqEq,
    Neq,
    Eq,
    Arrow,
    BigArrow,
    And,
    Or,
    Not,
    True,
    False,
    Let,
    Fun,
    If,
    Then,
    Else,
    Match,
    Def,
    Type,
    Record,
    Semi,
    Colon,
    ColonEq,
    Comma,
    Dot,
    End,
    Unknown,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            TokenKind::Number => "number",
            TokenKind::Identifier => "identifier",
            TokenKind::At => "`@`",
            TokenKind::Plus => "`+`",
            TokenKind::Star => "`*`",
            TokenKind::Dash => "`-`",
            TokenKind::Slash => "`/`",
            TokenKind::BSlash => "`\\`",
            TokenKind::Pipe => "`|`",
            TokenKind::LParen => "`(`",
            TokenKind::RParen => "`)`",
            TokenKind::LBrace => "`{`",
            TokenKind::RBrace => "`}`",
            TokenKind::Less => "`<`",
            TokenKind::LessEq => "`<=`",
            TokenKind::Greater => "`>`",
            TokenKind::GreaterEq => "`>=`",
            TokenKind::EqEq => "`==`",
            TokenKind::Neq => "`/=`",
            TokenKind::Eq => "`=`",
            TokenKind::Arrow => "`->`",
            TokenKind::BigArrow => "`=>`",
            TokenKind::And => "`and`",
            TokenKind::Or => "`or`",
            TokenKind::Not => "`not`",
            TokenKind::True => "`true`",
            TokenKind::False => "`false`",
            TokenKind::Let => "`let`",
            TokenKind::Fun => "`fun`",
            TokenKind::If => "`if`",
            TokenKind::Then => "`then`",
            TokenKind::Else => "`else`",
            TokenKind::Match => "`match`",
            TokenKind::Def => "`def`",
            TokenKind::Type => "`type`",
            TokenKind::Record => "`record`",
            TokenKind::Semi => "`;`",
            TokenKind::Colon => "`:`",
            TokenKind::ColonEq => "`:=`",
            TokenKind::Comma => "`,`",
            TokenKind::Dot => "`.`",
            TokenKind::End => "end of input",
            TokenKind::Unknown => "invalid character",
        };
        f.write_str(text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    file: u32,
    start: usize,
    end: usize,
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start + 1, self.end + 1)
    }
}

impl Span {
    pub fn new(file: u32) -> Span {
        Span {
            file,
            start: 0,
            end: 0,
        }
    }

    pub fn merge_with(&self, other: Span) -> Span {
        Span {
            file: self.file,
            start: std::cmp::min(self.start, other.start),
            end: std::cmp::max(self.end, other.end),
        }
    }
}

pub const HOST_FILE: u32 = u32::MAX;
