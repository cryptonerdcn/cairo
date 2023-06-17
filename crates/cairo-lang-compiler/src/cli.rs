use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use cairo_lang_compiler::{compile_cairo_project_at_path, compile_cairo_project_with_input_string , CompilerConfig};
use cairo_lang_utils::logging::init_logging;
use clap::Parser;
use salsa::plumbing::InputQueryStorageOps;

/// Command line args parser.
/// Exits with 0/1 if the input is formatted correctly/incorrectly.
#[derive(Parser, Debug)]
#[clap(version, verbatim_doc_comment)]
struct Args {
    /// The file to compile
    path: PathBuf,
    /// The input file string (default: stdin).
    input: Option<String>,
    /// The output file name (default: stdout).
    output: Option<String>,
    /// Replaces sierra ids with human-readable ones.
    #[arg(short, long, default_value_t = false)]
    replace_ids: bool,
}

fn main() -> anyhow::Result<()> {
    init_logging(log::LevelFilter::Warn);
    log::info!("Starting Cairo compilation.");

    let args = Args::parse();

    /*if args.input.is_some() {
        todo!("Input from string not yet supported.");
    }*/

    let sierra_program =  match args.input.as_ref() {
        Some(input) => {
            compile_cairo_project_with_input_string(&args.path, input, CompilerConfig {
                replace_ids: args.replace_ids,
                ..CompilerConfig::default()
            })?
        }
        None =>
            compile_cairo_project_at_path(&args.path, CompilerConfig {
                replace_ids: args.replace_ids,
                ..CompilerConfig::default()
            })?,
        };
    

    match args.output {
        Some(path) => {
            fs::write(path, format!("{sierra_program}")).context("Failed to write output.")?
        }
        None => println!("{sierra_program}"),
    }

    Ok(())
}
