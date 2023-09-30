use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    allowed_libfuncs::{validate_compatible_sierra_version, ListSelector},
    contract_class::{compile_contract_in_prepared_db, ContractClass},
    plugin::StarkNetPlugin,
};
use cairo_lang_compiler::{
    db::RootDatabase,
    diagnostics::{get_diagnostics_as_string, DiagnosticsReporter},
    wasm_cairo_interface::setup_project_with_input_string,
    CompilerConfig,
};

/// Compile Starknet crate (or specific contract in the crate).
pub fn starknet_compile_with_input_string(
    crate_path: PathBuf,
    contract_path: Option<String>,
    config: Option<CompilerConfig<'_>>,
    allowed_libfuncs_list: Option<ListSelector>,
    input_string: &String,
) -> anyhow::Result<String> {
    let contract = compile_path_with_input_string(
        &crate_path,
        contract_path.as_deref(),
        if let Some(config) = config { config } else { CompilerConfig::default() },
        input_string,
    )?;
    validate_compatible_sierra_version(
        &contract,
        if let Some(allowed_libfuncs_list) = allowed_libfuncs_list {
            allowed_libfuncs_list
        } else {
            ListSelector::default()
        },
    )?;
    serde_json::to_string_pretty(&contract).with_context(|| "Serialization failed.")
}

pub fn starknet_wasm_compile_with_input_string(
    input_program_string: &String,
    replace_ids: bool,
    contract_path: Option<String>,
    allowed_libfuncs_list_name: Option<String>,
    allowed_libfuncs_list_file: Option<String>,
) -> Result<String> {
    let list_selector = ListSelector::new(allowed_libfuncs_list_name, allowed_libfuncs_list_file)
        .expect("Both allowed libfunc list name and file were supplied.");

    let res = starknet_compile_with_input_string(
        Path::new("astro.cairo").to_path_buf(),
        contract_path,
        Some(CompilerConfig { replace_ids, ..CompilerConfig::default() }),
        Some(list_selector),
        input_program_string,
    )?;

    Ok(res)
}

/// Compile the contract given by path.
/// Errors if there is ambiguity.
pub fn compile_path_with_input_string(
    path: &Path,
    contract_path: Option<&str>,
    compiler_config: CompilerConfig<'_>,
    input_string: &String,
) -> Result<ContractClass> {
    let mut db = RootDatabase::builder()
        .detect_corelib()
        .with_semantic_plugin(Arc::new(StarkNetPlugin::default()))
        .build()?;

    let main_crate_ids = setup_project_with_input_string(&mut db, Path::new(&path), input_string)?;

    if DiagnosticsReporter::stderr().check(&db) {
        let err_string = get_diagnostics_as_string(&mut db);
        anyhow::bail!("failed to compile:\n {}", err_string);
    }

    compile_contract_in_prepared_db(&mut db, contract_path, main_crate_ids, compiler_config)
}
