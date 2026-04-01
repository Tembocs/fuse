#[derive(Debug, Clone, PartialEq)]
pub enum FStringPart {
    Str(String),
    Expr(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    // Keywords
    Fn, Val, Var, Ref, Mutref, Owned, Move,
    Struct, Class, Enum, Match, When,
    If, Else, For, In, Loop, Return, Defer,
    And, Or, Not, True, False, SelfKw,
    Spawn, Async, Await, Suspend,
    // Operators
    Arrow, FatArrow, QuestionDot, Elvis, Question,
    At, Dot, DotDot, Colon, ColonColon,
    Eq, EqEq, BangEq,
    Lt, Gt, LtEq, GtEq,
    Plus, Minus, Star, Slash, Percent, Pipe,
    // Delimiters
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Comma, Semicolon,
    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    FString(Vec<FStringPart>),
    // Identifiers
    Ident(String),
    // Special
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub ty: Tok,
    pub line: usize,
    pub col: usize,
}

pub fn keyword(s: &str) -> Option<Tok> {
    Some(match s {
        "fn" => Tok::Fn, "val" => Tok::Val, "var" => Tok::Var,
        "ref" => Tok::Ref, "mutref" => Tok::Mutref, "owned" => Tok::Owned,
        "move" => Tok::Move, "struct" => Tok::Struct, "class" => Tok::Class,
        "enum" => Tok::Enum, "match" => Tok::Match, "when" => Tok::When,
        "if" => Tok::If, "else" => Tok::Else, "for" => Tok::For,
        "in" => Tok::In, "loop" => Tok::Loop, "return" => Tok::Return,
        "defer" => Tok::Defer, "and" => Tok::And, "or" => Tok::Or,
        "not" => Tok::Not, "true" => Tok::True, "false" => Tok::False,
        "self" => Tok::SelfKw,
        "spawn" => Tok::Spawn, "async" => Tok::Async,
        "await" => Tok::Await, "suspend" => Tok::Suspend,
        _ => return None,
    })
}
