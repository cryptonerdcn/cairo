use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use cairo_lang_compiler::CompilerConfig;
use cairo_lang_compiler::db::RootDatabase;
use cairo_lang_compiler::wasm_cairo_interface::{
    DependencyInput, init_corelib, setup_project_with_input_string, setup_virtual_project,
    setup_virtual_project_with_deps,
};
use cairo_lang_defs::ids::TopLevelLanguageElementId;
use cairo_lang_filesystem::db::FilesGroup;
use cairo_lang_filesystem::ids::CrateInput;
use cairo_lang_starknet_classes::allowed_libfuncs::ListSelector;
use cairo_lang_starknet_classes::casm_contract_class::CasmContractClass;

use crate::compile::compile_contract_in_prepared_db;
use crate::contract::find_contracts;
use crate::starknet_plugin_suite;

/// Default max bytecode size for Sierra-to-CASM compilation (same as starknet-sierra-compile CLI).
const MAX_BYTECODE_SIZE: usize = 180000;

// ========== Single-file compilation (existing) ==========

/// Compile Starknet contract from input string.
pub fn starknet_wasm_compile_with_input_string(
    input_program_string: &str,
    allow_warnings: bool,
    replace_ids: bool,
    output_casm: bool,
    contract_path: Option<String>,
    allowed_libfuncs_list_name: Option<String>,
    allowed_libfuncs_list_file: Option<String>,
) -> Result<String> {
    let mut db =
        RootDatabase::builder().with_default_plugin_suite(starknet_plugin_suite()).build()?;
    init_corelib(&mut db);

    let main_crate_inputs =
        setup_project_with_input_string(&mut db, Path::new("astro.cairo"), input_program_string)?;

    compile_starknet_prepared(
        &db,
        &main_crate_inputs,
        allow_warnings,
        replace_ids,
        output_casm,
        contract_path,
        allowed_libfuncs_list_name,
        allowed_libfuncs_list_file,
    )
}

// ========== Multi-file compilation (new) ==========

/// Compile Starknet contract from a virtual multi-file project.
#[allow(clippy::too_many_arguments)]
pub fn starknet_wasm_compile_with_virtual_files(
    project_name: &str,
    files: &HashMap<String, String>,
    allow_warnings: bool,
    replace_ids: bool,
    output_casm: bool,
    contract_path: Option<String>,
    allowed_libfuncs_list_name: Option<String>,
    allowed_libfuncs_list_file: Option<String>,
) -> Result<String> {
    let mut db =
        RootDatabase::builder().with_default_plugin_suite(starknet_plugin_suite()).build()?;
    init_corelib(&mut db);

    let main_crate_inputs = setup_virtual_project(&mut db, project_name, files);

    compile_starknet_prepared(
        &db,
        &main_crate_inputs,
        allow_warnings,
        replace_ids,
        output_casm,
        contract_path,
        allowed_libfuncs_list_name,
        allowed_libfuncs_list_file,
    )
}

// ========== Multi-file compilation with dependencies (new) ==========

/// Compile Starknet contract from a virtual multi-file project with external dependencies.
#[allow(clippy::too_many_arguments)]
pub fn starknet_wasm_compile_with_virtual_files_and_deps(
    project_name: &str,
    files: &HashMap<String, String>,
    dependencies: &HashMap<String, DependencyInput>,
    allow_warnings: bool,
    replace_ids: bool,
    output_casm: bool,
    contract_path: Option<String>,
    allowed_libfuncs_list_name: Option<String>,
    allowed_libfuncs_list_file: Option<String>,
) -> Result<String> {
    let mut db =
        RootDatabase::builder().with_default_plugin_suite(starknet_plugin_suite()).build()?;
    init_corelib(&mut db);

    let main_crate_inputs =
        setup_virtual_project_with_deps(&mut db, project_name, files, dependencies);

    compile_starknet_prepared(
        &db,
        &main_crate_inputs,
        allow_warnings,
        replace_ids,
        output_casm,
        contract_path,
        allowed_libfuncs_list_name,
        allowed_libfuncs_list_file,
    )
}

// ========== Shared logic ==========

#[allow(clippy::too_many_arguments)]
fn compile_starknet_prepared(
    db: &RootDatabase,
    main_crate_inputs: &[CrateInput],
    allow_warnings: bool,
    replace_ids: bool,
    output_casm: bool,
    contract_path: Option<String>,
    allowed_libfuncs_list_name: Option<String>,
    allowed_libfuncs_list_file: Option<String>,
) -> Result<String> {
    let list_selector = ListSelector::new(allowed_libfuncs_list_name, allowed_libfuncs_list_file)
        .expect("Both allowed libfunc list name and file were supplied.");

    let main_crate_ids = CrateInput::into_crate_ids(db, main_crate_inputs.to_vec());

    // Debug: log registered crates and found contracts
    {
        let all_crates = db.crates();
        let crate_names: Vec<String> =
            all_crates.iter().map(|c| format!("{:?}", c.long(db))).collect();
        eprintln!("[starknet-compile] Registered crates: {:?}", crate_names);

        let contracts_found = find_contracts(db, &main_crate_ids);
        eprintln!("[starknet-compile] Found {} contracts in main crates", contracts_found.len());
        for c in &contracts_found {
            eprintln!("[starknet-compile]   contract: {:?}", c.submodule_id.full_path(db));
        }
    }

    // Pre-check: capture diagnostics for main crate (and its deps, excluding corelib) to a string
    {
        let mut diagnostics = String::new();
        let mut check_reporter =
            cairo_lang_compiler::diagnostics::DiagnosticsReporter::write_to_string(
                &mut diagnostics,
            )
            .with_crates(main_crate_inputs);
        if allow_warnings {
            check_reporter = check_reporter.allow_warnings();
        }
        let found = check_reporter.check(db);
        drop(check_reporter);
        if found {
            anyhow::bail!("failed to compile:\n{}", diagnostics);
        }
    }

    let mut diagnostics_reporter =
        cairo_lang_compiler::diagnostics::DiagnosticsReporter::callback(|_| ())
            .with_crates(main_crate_inputs);
    if allow_warnings {
        diagnostics_reporter = diagnostics_reporter.allow_warnings();
    }

    let contract = compile_contract_in_prepared_db(
        db,
        contract_path.as_deref(),
        main_crate_ids,
        CompilerConfig { replace_ids, diagnostics_reporter, ..CompilerConfig::default() },
    )?;

    let extracted_program = contract.extract_sierra_program(false)?;
    extracted_program.validate_version_compatible(list_selector)?;

    if output_casm {
        // Compile Sierra to CASM
        let casm_contract = CasmContractClass::from_contract_class(
            contract.clone(),
            contract.extract_sierra_program(false)?,
            true, // add_pythonic_hints
            MAX_BYTECODE_SIZE,
        )
        .map_err(|e| anyhow::anyhow!("Sierra to CASM compilation failed: {}", e))?;

        let sierra_json =
            serde_json::to_value(&contract).with_context(|| "Sierra serialization failed.")?;
        let casm_json =
            serde_json::to_value(&casm_contract).with_context(|| "CASM serialization failed.")?;

        let result = serde_json::json!({
            "sierra": sierra_json,
            "casm": casm_json,
        });
        serde_json::to_string_pretty(&result).with_context(|| "Serialization failed.")
    } else {
        serde_json::to_string_pretty(&contract).with_context(|| "Serialization failed.")
    }
}
