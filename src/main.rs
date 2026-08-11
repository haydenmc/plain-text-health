mod lexer;
mod parser;
mod directives;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
    }
}

fn main() {
    let args = Cli::parse();
    let (name, file) = match &args.command {
        Command::Check { file } => ("check", file),
    };
    println!("command: {}, file: {}", name, file.display());
}
