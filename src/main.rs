mod directives;
mod lexer;
mod parser;

use std::{fs, path::PathBuf, process::{ExitCode, ExitStatus, exit}};

use clap::{Parser, Subcommand, error};

use crate::{lexer::Token::Comma, parser::parse};

#[derive(Parser, Debug)]
#[command(name = "fitlog", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Check {
        #[arg(env = "FITLOG_FILE")]
        file: PathBuf,
    },
}

fn check(file: &PathBuf) -> ExitCode {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fitlog: could not read `{}`: {}", file.display(), e);
            return ExitCode::FAILURE;
        }
    };

    let (directives, errors) = parse(&src);
    for err in &errors {
        eprintln!("Parse Error at `{}`: {}", &src[err.span.clone()], err.msg);
    }
    if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn main() -> ExitCode {
    env_logger::init();

    let args = Cli::parse();

    match &args.command {
        Command::Check { file } => {
            check(file)
        }
    }
}
