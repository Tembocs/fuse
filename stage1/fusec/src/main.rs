mod error;
mod lexer;
mod ast;
mod parser;
mod checker;
mod hir;
mod codegen;
mod eval;

use std::{env, fs, process};
use lexer::Lexer;
use parser::Parser;
use checker::Checker;
use eval::Evaluator;

const VERSION: &str = "0.1.0";

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        println!("fusec {} — The Fuse compiler", VERSION);
        println!();
        println!("Usage: fusec [options] <file.fuse>");
        println!();
        println!("Options:");
        println!("  <file.fuse>       Run a Fuse source file");
        println!("  --check <file>    Check for errors without running");
        println!("  --version, -v     Print version");
        println!("  --help, -h        Print this help");
        if args.len() < 2 { process::exit(1); }
        return;
    }

    if args[1] == "--version" || args[1] == "-v" {
        println!("fusec {VERSION}");
        return;
    }

    let (mode, filepath) = if args[1] == "--check" && args.len() > 2 {
        ("check", &args[2])
    } else {
        ("run", &args[1])
    };

    let source = match fs::read_to_string(filepath) {
        Ok(s) => s,
        Err(e) => { eprintln!("error: {e}"); process::exit(1); }
    };

    run(mode, &source, filepath);
}

fn run(mode: &str, source: &str, filepath: &str) {
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

    for d in &diagnostics {
        eprintln!("{d}");
    }

    let has_errors = diagnostics.iter().any(|e| e.kind == error::ErrorKind::Error);
    if has_errors {
        process::exit(1);
    }

    if mode == "check" { return; }

    // Run — tree-walking evaluation using fuse-runtime
    let mut evaluator = Evaluator::new(program, display_name);
    evaluator.run();
}
