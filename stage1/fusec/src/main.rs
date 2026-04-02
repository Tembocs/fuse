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
use hir::lower::Lowerer;
use codegen::cranelift::Codegen;

const VERSION: &str = "0.1.0";

fn main() {
    // Spawn the real main on a thread with a large stack to support
    // deep recursion in interpreted Fuse programs (Stage 2 self-hosting).
    let builder = std::thread::Builder::new().stack_size(128 * 1024 * 1024);
    let handler = builder.spawn(real_main).unwrap();
    if let Err(e) = handler.join() {
        if let Some(msg) = e.downcast_ref::<&str>() {
            eprintln!("{msg}");
        } else if let Some(msg) = e.downcast_ref::<String>() {
            eprintln!("{msg}");
        }
        process::exit(1);
    }
}

fn real_main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        println!("fusec {} — The Fuse compiler", VERSION);
        println!();
        println!("Usage: fusec [options] <file.fuse>");
        println!();
        println!("Options:");
        println!("  <file.fuse>               Run a Fuse source file (interpreter)");
        println!("  --compile <file> [-o out]  Compile to native binary");
        println!("  --check <file>            Check for errors without running");
        println!("  --version, -v              Print version");
        println!("  --help, -h                 Print this help");
        if args.len() < 2 { process::exit(1); }
        return;
    }

    if args[1] == "--version" || args[1] == "-v" {
        println!("fusec {VERSION}");
        return;
    }

    let (mode, filepath, output) = if args[1] == "--check" && args.len() > 2 {
        ("check", &args[2], String::new())
    } else if args[1] == "--compile" && args.len() > 2 {
        let out = if args.len() > 4 && args[3] == "-o" {
            args[4].clone()
        } else {
            args[2].trim_end_matches(".fuse").to_string()
        };
        ("compile", &args[2], out)
    } else {
        ("run", &args[1], String::new())
    };

    let source = match fs::read_to_string(filepath) {
        Ok(s) => s,
        Err(e) => { eprintln!("error: {e}"); process::exit(1); }
    };

    run(mode, &source, filepath, &output);
}

fn run(mode: &str, source: &str, filepath: &str, output: &str) {
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

    if mode == "compile" {
        // Compile — lower to HIR and generate native binary via Cranelift.
        let mut lowerer = Lowerer::new();
        let hir_program = lowerer.lower(&program);
        let cg = Codegen::new();
        cg.compile(&hir_program, output);
        return;
    }

    // Run — tree-walking evaluation using fuse-runtime
    let mut evaluator = Evaluator::new(program, display_name);
    evaluator.run();
}
