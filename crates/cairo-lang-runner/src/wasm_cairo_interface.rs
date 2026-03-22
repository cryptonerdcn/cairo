use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Error, Result};
use cairo_lang_compiler::db::RootDatabase;
use cairo_lang_compiler::diagnostics::DiagnosticsReporter;
use cairo_lang_compiler::wasm_cairo_interface::{
    DependencyInput, init_corelib, setup_project_with_input_string, setup_virtual_project,
    setup_virtual_project_with_deps,
};
use cairo_lang_diagnostics::ToOption;
use cairo_lang_filesystem::ids::CrateInput;
use cairo_lang_sierra_generator::db::SierraGenGroup;
use cairo_lang_sierra_generator::program_generator::SierraProgramWithDebug;
use cairo_lang_sierra_generator::replace_ids::{DebugReplacer, SierraIdReplacer};
use cairo_lang_starknet::contract::{find_contracts, get_contracts_info};

use crate::casm_run::format_next_item;
use crate::{
    ProfilingInfoCollectionConfig, RunResultStarknet, RunResultValue, SierraCasmRunner,
    StarknetState,
};

// ========== Single-file run (existing) ==========

pub fn run_with_input_program_string(
    input_program_string: &str,
    available_gas: Option<usize>,
    allow_warnings: bool,
    print_full_memory: bool,
    run_profiler: bool,
    use_dbg_print_hint: bool,
) -> Result<String> {
    let path = Path::new("astro.cairo");

    let mut db_builder = RootDatabase::builder();
    if available_gas.is_none() {
        db_builder.skip_auto_withdraw_gas();
    }
    let db = &mut db_builder.build()?;
    init_corelib(db);

    let main_crate_inputs = setup_project_with_input_string(db, path, input_program_string)?;
    run_prepared_project(
        db,
        main_crate_inputs,
        available_gas,
        allow_warnings,
        print_full_memory,
        run_profiler,
        use_dbg_print_hint,
    )
}

// ========== Multi-file run (new) ==========

pub fn run_with_virtual_project(
    project_name: &str,
    files: &HashMap<String, String>,
    available_gas: Option<usize>,
    allow_warnings: bool,
    print_full_memory: bool,
    run_profiler: bool,
    use_dbg_print_hint: bool,
) -> Result<String> {
    let mut db_builder = RootDatabase::builder();
    if available_gas.is_none() {
        db_builder.skip_auto_withdraw_gas();
    }
    let db = &mut db_builder.build()?;
    init_corelib(db);

    let main_crate_inputs = setup_virtual_project(db, project_name, files);
    run_prepared_project(
        db,
        main_crate_inputs,
        available_gas,
        allow_warnings,
        print_full_memory,
        run_profiler,
        use_dbg_print_hint,
    )
}

// ========== Multi-file run with dependencies (new) ==========

#[allow(clippy::too_many_arguments)]
pub fn run_with_virtual_project_and_deps(
    project_name: &str,
    files: &HashMap<String, String>,
    dependencies: &HashMap<String, DependencyInput>,
    available_gas: Option<usize>,
    allow_warnings: bool,
    print_full_memory: bool,
    run_profiler: bool,
    use_dbg_print_hint: bool,
) -> Result<String> {
    let mut db_builder = RootDatabase::builder();
    if available_gas.is_none() {
        db_builder.skip_auto_withdraw_gas();
    }
    let db = &mut db_builder.build()?;
    init_corelib(db);

    let main_crate_inputs = setup_virtual_project_with_deps(db, project_name, files, dependencies);
    run_prepared_project(
        db,
        main_crate_inputs,
        available_gas,
        allow_warnings,
        print_full_memory,
        run_profiler,
        use_dbg_print_hint,
    )
}

// ========== Shared logic ==========

fn run_prepared_project(
    db: &mut RootDatabase,
    main_crate_inputs: Vec<CrateInput>,
    available_gas: Option<usize>,
    allow_warnings: bool,
    print_full_memory: bool,
    run_profiler: bool,
    use_dbg_print_hint: bool,
) -> Result<String> {
    let main_crate_ids = CrateInput::into_crate_ids(db, main_crate_inputs.clone());

    {
        let mut diagnostics = String::new();
        let mut reporter =
            DiagnosticsReporter::write_to_string(&mut diagnostics).with_crates(&main_crate_inputs);
        if allow_warnings {
            reporter = reporter.allow_warnings();
        }
        let found = reporter.check(db);
        drop(reporter);
        if found {
            return Err(anyhow::anyhow!("failed to compile:\n {}", diagnostics));
        }
    }

    let SierraProgramWithDebug { program: mut sierra_program, debug_info: _ } = db
        .get_sierra_program(main_crate_ids.clone())
        .to_option()
        .with_context(|| "Compilation failed without any diagnostics.")?
        .clone();
    let replacer = DebugReplacer { db };
    replacer.enrich_function_names(&mut sierra_program);
    if available_gas.is_none() && sierra_program.requires_gas_counter() {
        anyhow::bail!("Program requires gas counter, please provide `--available-gas` argument.");
    }

    let contracts = find_contracts(db, &main_crate_ids);
    let contracts_info = get_contracts_info(db, contracts, &replacer)?;
    let sierra_program = replacer.apply(&sierra_program);

    let runner = SierraCasmRunner::new(
        sierra_program.clone(),
        if available_gas.is_some() { Some(Default::default()) } else { None },
        contracts_info,
        if run_profiler { Some(ProfilingInfoCollectionConfig::default()) } else { None },
    )
    .map_err(|err| Error::msg(err.to_string()))?;

    let func = runner
        .find_function("::main")
        .map_err(|err: crate::RunnerError| Error::msg(err.to_string()))?;
    let result = runner
        .run_function_with_starknet_context(func, vec![], available_gas, StarknetState::default())
        .map_err(|err: crate::RunnerError| Error::msg(err.to_string()))?;

    generate_run_result_log(&result, print_full_memory, use_dbg_print_hint)
}

fn generate_run_result_log(
    result: &RunResultStarknet,
    print_full_memory: bool,
    _use_dbg_print_hint: bool,
) -> Result<String> {
    let mut result_string = String::new();

    match &result.value {
        RunResultValue::Success(values) => {
            result_string.push_str(&format!("Run completed successfully, returning {values:?}\n"));
        }
        RunResultValue::Panic(values) => {
            result_string.push_str("Run panicked with [");
            let mut felts = values.clone().into_iter();
            let mut first = true;
            while let Some(item) = format_next_item(&mut felts) {
                if !first {
                    result_string.push_str(", ");
                }
                first = false;
                result_string.push_str(&item.quote_if_string());
            }
            result_string.push_str("].\n");
        }
    }
    if let Some(gas) = &result.gas_counter {
        result_string.push_str(&format!("Remaining gas: {gas}\n"));
    }
    if print_full_memory {
        result_string.push_str("Full memory: [");
        for cell in &result.memory {
            match cell {
                None => result_string.push_str("_, "),
                Some(value) => result_string.push_str(&format!("{value}, ")),
            }
        }
        result_string.push_str("]\n");
    }
    Ok(result_string)
}
