use std::path::Path;

use anyhow::{Context, Result};
use cairo_lang_compiler::diagnostics::get_diagnostics_as_string;
use cairo_lang_compiler::{wasm_cairo_interface::setup_project_with_input_string, db::RootDatabase, diagnostics::DiagnosticsReporter};
use cairo_lang_diagnostics::ToOption;
use cairo_lang_sierra::extensions::gas::{
    BuiltinCostWithdrawGasLibfunc, RedepositGasLibfunc, WithdrawGasLibfunc,
};
use cairo_lang_sierra::extensions::NamedLibfunc;
use cairo_lang_sierra_generator::db::SierraGenGroup;
use cairo_lang_sierra_generator::replace_ids::{DebugReplacer, SierraIdReplacer};
use cairo_lang_starknet::contract::get_contracts_info;
use cairo_lang_filesystem::log_db::LogDatabase;

use crate::short_string::as_cairo_short_string;
use crate::{SierraCasmRunner, StarknetState, RunResult, RunResultValue};

pub fn run_with_input_program_string(
    input_program_string: &String,
    available_gas: Option<usize>,
    print_full_memory: bool,
    use_dbg_print_hint: bool,
) -> Result<String> {
    let db = &mut RootDatabase::builder().detect_corelib().build()?;

    let main_crate_ids = setup_project_with_input_string(db, Path::new("astro.cairo"), &input_program_string)?;

    if DiagnosticsReporter::stderr().check(db) {
        let err_string = get_diagnostics_as_string(db);
        anyhow::bail!("failed to compile:\n {}", err_string);
    }

    let sierra_program = db
        .get_sierra_program(main_crate_ids.clone())
        .to_option()
        .with_context(|| "Compilation failed without any diagnostics.")?;
    let replacer = DebugReplacer { db };
    if available_gas.is_none()
        && sierra_program.type_declarations.iter().any(|decl| {
            matches!(
                decl.long_id.generic_id.0.as_str(),
                WithdrawGasLibfunc::STR_ID
                    | BuiltinCostWithdrawGasLibfunc::STR_ID
                    | RedepositGasLibfunc::STR_ID
            )
        })
    {
        anyhow::bail!("Program requires gas counter, please provide `--available_gas` argument.");
    }

    let contracts_info = get_contracts_info(db, main_crate_ids, &replacer)?;

    let runner = SierraCasmRunner::new(
        replacer.apply(&sierra_program),
        if available_gas.is_some() { Some(Default::default()) } else { None },
        contracts_info,
    )
    .with_context(|| "Failed setting up runner.")?;
    let result = runner
        .run_function(
            runner.find_function("::main")?,
            &[],
            available_gas,
            StarknetState::default(),
        )
        .with_context(|| "Failed to run the function.")?;
    generate_run_result_log(&result, print_full_memory, use_dbg_print_hint)
}

fn generate_run_result_log(
    result: &RunResult,
    print_full_memory: bool,
    use_dbg_print_hint: bool,
) -> Result<String> {
    let mut result_string = String::new();

    if use_dbg_print_hint {
        println!("{}\n", LogDatabase::get_file_text("log_file".to_string()));
        result_string.push_str(&format!("{}", LogDatabase::get_file_text("log_file".to_string())));
    }

    match &result.value {
        RunResultValue::Success(values) => {
            println!("Run completed successfully, returning {values:?}");
            result_string.push_str(&format!(
                "Run completed successfully, returning {values:?}\n",
                values = values
            ))
        }
        RunResultValue::Panic(values) => {
            print!("Run panicked with [");
            result_string.push_str(&format!("Run panicked with ["));
            for value in values {
                match as_cairo_short_string(value) {
                    Some(as_string) => {
                        print!("{value} ('{as_string}'), ");
                        result_string.push_str(&format!(
                            "{value} ('{as_string}'), ",
                            value = value,
                            as_string = as_string
                        ));
                    }
                    None => {
                        print!("{value}, ");
                        result_string.push_str(&format!("{value}, ", value = value))
                    }
                }
            }
            println!("].");
            result_string.push_str(&format!("].\n"))
        }
    }
    if let Some(gas) = &result.gas_counter {
        println!("Remaining gas: {gas}");
        result_string.push_str(&format!("Remaining gas: {gas}\n", gas = gas));
    }
    if print_full_memory {
        print!("Full memory: [");
        result_string.push_str(&format!("Full memory: ["));
        for cell in &result.memory {
            match cell {
                None => {
                    print!("_, ");
                    result_string.push_str(&format!("_, "))
                }
                Some(value) => {
                    print!("{value}, ");
                    result_string.push_str(&format!("{value}, ", value = value))
                }
            }
        }
        result_string.push_str(&format!("]\n"))
    }
    Ok(result_string)
}
