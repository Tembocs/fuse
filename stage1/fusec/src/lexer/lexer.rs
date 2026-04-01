use crate::error::FuseError;
use super::token::*;

pub struct Lexer {
    src: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    file: String,
}

impl Lexer {
    pub fn new(source: &str, file: &str) -> Self {
        Self { src: source.chars().collect(), pos: 0, line: 1, col: 1, file: file.into() }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, FuseError> {
        let mut tokens = Vec::new();
        loop {
            self.skip_ws();
            if self.pos >= self.src.len() { break; }
            let ch = self.cur();
            if ch.is_ascii_digit() {
                tokens.push(self.number()?);
            } else if ch == '"' {
                tokens.push(self.string()?);
            } else if ch == 'f' && self.peek(1) == Some('"') {
                tokens.push(self.fstring()?);
            } else if ch.is_ascii_alphabetic() || ch == '_' {
                tokens.push(self.ident());
            } else {
                tokens.push(self.operator()?);
            }
        }
        tokens.push(Token { ty: Tok::Eof, line: self.line, col: self.col });
        Ok(tokens)
    }

    // ── helpers ──────────────────────────────────────────────────────
    fn cur(&self) -> char { self.src[self.pos] }
    fn peek(&self, off: usize) -> Option<char> { self.src.get(self.pos + off).copied() }

    fn advance(&mut self) -> char {
        let ch = self.src[self.pos];
        self.pos += 1;
        if ch == '\n' { self.line += 1; self.col = 1; } else { self.col += 1; }
        ch
    }

    fn err(&self, msg: impl Into<String>) -> FuseError {
        FuseError::new(msg, &self.file, self.line, self.col)
    }

    // ── whitespace / comments ────────────────────────────────────────
    fn skip_ws(&mut self) {
        while self.pos < self.src.len() {
            let ch = self.cur();
            if ch == ' ' || ch == '\t' || ch == '\r' || ch == '\n' {
                self.advance();
            } else if ch == '/' && self.peek(1) == Some('/') {
                while self.pos < self.src.len() && self.cur() != '\n' { self.advance(); }
            } else {
                break;
            }
        }
    }

    // ── numbers ──────────────────────────────────────────────────────
    fn number(&mut self) -> Result<Token, FuseError> {
        let (sl, sc) = (self.line, self.col);
        let mut s = String::new();
        let mut is_float = false;
        while self.pos < self.src.len() {
            let ch = self.cur();
            if ch.is_ascii_digit() {
                s.push(self.advance());
            } else if ch == '.' {
                if self.peek(1) == Some('.') { break; }
                if self.peek(1).map_or(false, |c| c.is_ascii_alphabetic() || c == '_') { break; }
                is_float = true;
                s.push(self.advance());
            } else {
                break;
            }
        }
        let ty = if is_float {
            Tok::Float(s.parse::<f64>().map_err(|_| self.err("invalid float"))?)
        } else {
            Tok::Int(s.parse::<i64>().map_err(|_| self.err("invalid int"))?)
        };
        Ok(Token { ty, line: sl, col: sc })
    }

    // ── strings ──────────────────────────────────────────────────────
    fn string(&mut self) -> Result<Token, FuseError> {
        let (sl, sc) = (self.line, self.col);
        self.advance(); // skip "
        let s = self.string_body()?;
        Ok(Token { ty: Tok::Str(s), line: sl, col: sc })
    }

    fn string_body(&mut self) -> Result<String, FuseError> {
        let mut s = String::new();
        while self.pos < self.src.len() && self.cur() != '"' {
            if self.cur() == '\\' {
                self.advance();
                s.push(self.escape()?);
            } else {
                s.push(self.advance());
            }
        }
        if self.pos >= self.src.len() { return Err(self.err("unterminated string")); }
        self.advance(); // skip closing "
        Ok(s)
    }

    fn escape(&mut self) -> Result<char, FuseError> {
        if self.pos >= self.src.len() { return Err(self.err("unterminated escape")); }
        let ch = self.advance();
        Ok(match ch {
            'n' => '\n', 't' => '\t', '\\' => '\\', '"' => '"',
            '{' => '{', '}' => '}',
            _ => ch,
        })
    }

    // ── f-strings ────────────────────────────────────────────────────
    fn fstring(&mut self) -> Result<Token, FuseError> {
        let (sl, sc) = (self.line, self.col);
        self.advance(); // f
        self.advance(); // "
        let mut parts = Vec::new();
        let mut buf = String::new();
        while self.pos < self.src.len() && self.cur() != '"' {
            if self.cur() == '{' {
                if !buf.is_empty() { parts.push(FStringPart::Str(std::mem::take(&mut buf))); }
                self.advance();
                let expr = self.fstring_expr()?;
                parts.push(FStringPart::Expr(expr));
            } else if self.cur() == '\\' {
                self.advance();
                buf.push(self.escape()?);
            } else {
                buf.push(self.advance());
            }
        }
        if !buf.is_empty() { parts.push(FStringPart::Str(buf)); }
        if self.pos >= self.src.len() { return Err(self.err("unterminated f-string")); }
        self.advance(); // "
        Ok(Token { ty: Tok::FString(parts), line: sl, col: sc })
    }

    fn fstring_expr(&mut self) -> Result<String, FuseError> {
        let mut s = String::new();
        let mut depth = 1u32;
        while self.pos < self.src.len() && depth > 0 {
            let ch = self.cur();
            if ch == '{' { depth += 1; }
            if ch == '}' { depth -= 1; }
            if depth > 0 { s.push(self.advance()); } else { self.advance(); }
        }
        if depth > 0 { return Err(self.err("unterminated f-string interpolation")); }
        Ok(s)
    }

    // ── identifiers / keywords ───────────────────────────────────────
    fn ident(&mut self) -> Token {
        let (sl, sc) = (self.line, self.col);
        let mut s = String::new();
        while self.pos < self.src.len() && (self.cur().is_ascii_alphanumeric() || self.cur() == '_') {
            s.push(self.advance());
        }
        let ty = keyword(&s).unwrap_or_else(|| Tok::Ident(s));
        Token { ty, line: sl, col: sc }
    }

    // ── operators / delimiters ───────────────────────────────────────
    fn operator(&mut self) -> Result<Token, FuseError> {
        let (sl, sc) = (self.line, self.col);
        let c1 = self.cur();
        let c2 = self.peek(1);
        let two: String = if let Some(c) = c2 { format!("{c1}{c}") } else { String::new() };

        let two_ty = match two.as_str() {
            "=>" => Some(Tok::FatArrow), "->" => Some(Tok::Arrow),
            "?." => Some(Tok::QuestionDot), "?:" => Some(Tok::Elvis),
            "==" => Some(Tok::EqEq), "!=" => Some(Tok::BangEq),
            "<=" => Some(Tok::LtEq), ">=" => Some(Tok::GtEq),
            "::" => Some(Tok::ColonColon), ".." => Some(Tok::DotDot),
            _ => None,
        };
        if let Some(ty) = two_ty {
            self.advance(); self.advance();
            return Ok(Token { ty, line: sl, col: sc });
        }

        let one_ty = match c1 {
            '?' => Tok::Question, '@' => Tok::At, '.' => Tok::Dot,
            ':' => Tok::Colon, '=' => Tok::Eq, '<' => Tok::Lt, '>' => Tok::Gt,
            '+' => Tok::Plus, '-' => Tok::Minus, '*' => Tok::Star,
            '/' => Tok::Slash, '%' => Tok::Percent, '|' => Tok::Pipe,
            '(' => Tok::LParen, ')' => Tok::RParen,
            '{' => Tok::LBrace, '}' => Tok::RBrace,
            '[' => Tok::LBracket, ']' => Tok::RBracket,
            ',' => Tok::Comma, ';' => Tok::Semicolon,
            _ => return Err(self.err(format!("unexpected character: {c1:?}"))),
        };
        self.advance();
        Ok(Token { ty: one_ty, line: sl, col: sc })
    }
}
