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

    run(mode, &source, filepath);
}

fn run(_mode: &str, source: &str, filepath: &str) {
    // Use just the filename for error messages
    let display_name = std::path::Path::new(filepath)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(filepath);

    // Lex
    let mut lexer = Lexer::new(source, display_name);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(e) => { eprintln!("{e}"); process::exit(1); }
    };

    // Parse
    let mut parser = Parser::new(tokens, display_name);
    let program = match parser.parse() {
        Ok(p) => p,
        Err(e) => { eprintln!("{e}"); process::exit(1); }
    };

    // Check
    let checker = Checker::new(&program, display_name);
    let diagnostics = checker.check(&program);

    // Print all diagnostics
    for d in &diagnostics {
        eprintln!("{d}");
    }

    // Only true errors cause failure (not warnings)
    let has_errors = diagnostics.iter().any(|e| e.kind == error::ErrorKind::Error);
    if has_errors {
        process::exit(1);
    }

    // Phase 6: check-only — no code generation yet
}
