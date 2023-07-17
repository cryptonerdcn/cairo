use std::path::{PathBuf, Path};
use anyhow::{Result, Context};

use cairo_lang_compiler::CompilerConfig;
use crate::{contract_class::compile_path_with_input_string, allowed_libfuncs::{ListSelector, validate_compatible_sierra_version}};

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