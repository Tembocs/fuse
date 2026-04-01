mod error;
mod lexer;
mod ast;
mod parser;
mod checker;
mod hir;
mod codegen;

use std::{env, fs, process};
use error::FuseError;
use lexer::Lexer;
use parser::Parser;
use checker::Checker;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: fusec [--check] <file.fuse>");
        process::exit(1);
    }

    let (mode, filepath) = if args[1] == "--check" && args.len() > 2 {
        ("check", &args[2])
    } else {
        ("check", &args[1]) // Phase 6: check-only is the default
    };

    let source = match fs::read_to_string(filepath) {
        Ok(s) => s,
        Err(e) => { eprintln!("error: {e}"); process::exit(1); }
    };

    if let Err(e) = run(mode, &source, filepath) {
        eprintln!("{e}");
        process::exit(1);
    }
}

fn run(_mode: &str, source: &str, filepath: &str) -> Result<(), FuseError> {
    // Lex
    let mut lexer = Lexer::new(source, filepath);
    let tokens = lexer.tokenize()?;

    // Parse
    let mut parser = Parser::new(tokens, filepath);
    let program = parser.parse()?;

    // Check
    let checker = Checker::new(&program, filepath);
    let errors = checker.check(&program);
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("{e}");
        }
        return Err(errors.into_iter().next().unwrap());
    }

    // Phase 6: check-only — no code generation yet
    Ok(())
}
