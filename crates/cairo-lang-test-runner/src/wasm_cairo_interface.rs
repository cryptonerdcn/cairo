use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use cairo_lang_compiler::db::RootDatabase;
use cairo_lang_compiler::wasm_cairo_interface::{
    DependencyInput, init_corelib, setup_project_with_input_string, setup_virtual_project,
    setup_virtual_project_with_deps,
};
use cairo_lang_filesystem::cfg::{Cfg, CfgSet};
use cairo_lang_starknet::starknet_plugin_suite;
use cairo_lang_test_plugin::{TestsCompilationConfig, test_plugin_suite};

use crate::{TestCompiler, TestRunConfig, TestRunner, TestsSummary};

// ========== Single-file test runner (existing) ==========

impl<'db> TestRunner<'db> {
    /// Configure a new test runner with string input
    pub fn new_with_string(
        input_program_string: &str,
        path: &Path,
        starknet: bool,
        allow_warnings: bool,
        config: TestRunConfig,
    ) -> Result<Self> {
        let compiler = TestCompiler::try_new_with_string(
            input_program_string,
            path,
            allow_warnings,
            config.gas_enabled,
            TestsCompilationConfig {
                starknet,
                add_statements_functions: config
                    .profiler_config
                    .as_ref()
                    .is_some_and(|c| c.requires_cairo_debug_info()),
                add_statements_code_locations: false,
                contract_declarations: None,
                contract_crate_ids: None,
                executable_crate_ids: None,
                add_functions_debug_info: false,
                replace_ids: false,
            },
        )?;
        Ok(Self { compiler, config, custom_hint_processor_factory: None })
    }

    /// Configure a new test runner with a virtual multi-file project
    pub fn new_with_virtual_project(
        project_name: &str,
        files: &HashMap<String, String>,
        starknet: bool,
        allow_warnings: bool,
        config: TestRunConfig,
    ) -> Result<Self> {
        let compiler = TestCompiler::try_new_with_virtual_project(
            project_name,
            files,
            allow_warnings,
            config.gas_enabled,
            TestsCompilationConfig {
                starknet,
                add_statements_functions: config
                    .profiler_config
                    .as_ref()
                    .is_some_and(|c| c.requires_cairo_debug_info()),
                add_statements_code_locations: false,
                contract_declarations: None,
                contract_crate_ids: None,
                executable_crate_ids: None,
                add_functions_debug_info: false,
                replace_ids: false,
            },
        )?;
        Ok(Self { compiler, config, custom_hint_processor_factory: None })
    }

    /// Configure a new test runner with a virtual multi-file project and external dependencies
    pub fn new_with_virtual_project_and_deps(
        project_name: &str,
        files: &HashMap<String, String>,
        dependencies: &HashMap<String, DependencyInput>,
        starknet: bool,
        allow_warnings: bool,
        config: TestRunConfig,
    ) -> Result<Self> {
        let compiler = TestCompiler::try_new_with_virtual_project_and_deps(
            project_name,
            files,
            dependencies,
            allow_warnings,
            config.gas_enabled,
            TestsCompilationConfig {
                starknet,
                add_statements_functions: config
                    .profiler_config
                    .as_ref()
                    .is_some_and(|c| c.requires_cairo_debug_info()),
                add_statements_code_locations: false,
                contract_declarations: None,
                contract_crate_ids: None,
                executable_crate_ids: None,
                add_functions_debug_info: false,
                replace_ids: false,
            },
        )?;
        Ok(Self { compiler, config, custom_hint_processor_factory: None })
    }
}

impl<'db> TestCompiler<'db> {
    /// Configure a new test compiler with string input
    pub fn try_new_with_string(
        input_program_string: &str,
        path: &Path,
        allow_warnings: bool,
        gas_enabled: bool,
        config: TestsCompilationConfig<'db>,
    ) -> Result<Self> {
        let mut db = build_test_db(gas_enabled, config.starknet)?;
        let main_crate_inputs =
            setup_project_with_input_string(&mut db, path, input_program_string)?;

        Ok(Self {
            db: db.snapshot(),
            test_crate_ids: main_crate_inputs.clone(),
            main_crate_ids: main_crate_inputs,
            allow_warnings,
            config,
        })
    }

    /// Configure a new test compiler with a virtual multi-file project
    pub fn try_new_with_virtual_project(
        project_name: &str,
        files: &HashMap<String, String>,
        allow_warnings: bool,
        gas_enabled: bool,
        config: TestsCompilationConfig<'db>,
    ) -> Result<Self> {
        let mut db = build_test_db(gas_enabled, config.starknet)?;
        let main_crate_inputs = setup_virtual_project(&mut db, project_name, files);

        Ok(Self {
            db: db.snapshot(),
            test_crate_ids: main_crate_inputs.clone(),
            main_crate_ids: main_crate_inputs,
            allow_warnings,
            config,
        })
    }

    /// Configure a new test compiler with a virtual multi-file project and external dependencies
    pub fn try_new_with_virtual_project_and_deps(
        project_name: &str,
        files: &HashMap<String, String>,
        dependencies: &HashMap<String, DependencyInput>,
        allow_warnings: bool,
        gas_enabled: bool,
        config: TestsCompilationConfig<'db>,
    ) -> Result<Self> {
        let mut db = build_test_db(gas_enabled, config.starknet)?;
        let main_crate_inputs =
            setup_virtual_project_with_deps(&mut db, project_name, files, dependencies);

        Ok(Self {
            db: db.snapshot(),
            test_crate_ids: main_crate_inputs.clone(),
            main_crate_ids: main_crate_inputs,
            allow_warnings,
            config,
        })
    }
}

/// Build a RootDatabase configured for test execution.
fn build_test_db(gas_enabled: bool, starknet: bool) -> Result<RootDatabase> {
    let mut b = RootDatabase::builder();
    let mut cfg = CfgSet::from_iter([Cfg::name("test"), Cfg::kv("target", "test")]);
    if !gas_enabled {
        cfg.insert(Cfg::kv("gas", "disabled"));
        b.skip_auto_withdraw_gas();
    }
    b.with_cfg(cfg);
    b.with_default_plugin_suite(test_plugin_suite());
    if starknet {
        b.with_default_plugin_suite(starknet_plugin_suite());
    }
    let mut db = b.build()?;
    init_corelib(&mut db);
    Ok(db)
}

// ========== Single-file test execution ==========

#[allow(clippy::too_many_arguments)]
pub fn run_tests_with_input_string(
    input_program_string: &str,
    allow_warnings: bool,
    filter: String,
    include_ignored: bool,
    ignored: bool,
    starknet: bool,
    _run_profiler: String,
    gas_disabled: bool,
    print_resource_usage: bool,
) -> Result<Option<TestsSummary>> {
    let path = Path::new("astro");
    let config = TestRunConfig {
        filter,
        ignored,
        include_ignored,
        profiler_config: None,
        gas_enabled: !gas_disabled,
        print_resource_usage,
    };

    let runner =
        TestRunner::new_with_string(input_program_string, path, starknet, allow_warnings, config)?;
    runner.run()
}

#[allow(clippy::too_many_arguments)]
pub fn run_tests_with_input_string_parsed(
    input_program_string: &str,
    allow_warnings: bool,
    filter: String,
    include_ignored: bool,
    ignored: bool,
    starknet: bool,
    run_profiler: String,
    gas_disabled: bool,
    print_resource_usage: bool,
) -> Result<String> {
    let result = run_tests_with_input_string(
        input_program_string,
        allow_warnings,
        filter,
        include_ignored,
        ignored,
        starknet,
        run_profiler,
        gas_disabled,
        print_resource_usage,
    );
    format_test_result(result)
}

// ========== Multi-file test execution (new) ==========

#[allow(clippy::too_many_arguments)]
pub fn run_tests_with_virtual_project(
    project_name: &str,
    files: &HashMap<String, String>,
    allow_warnings: bool,
    filter: String,
    include_ignored: bool,
    ignored: bool,
    starknet: bool,
    gas_disabled: bool,
    print_resource_usage: bool,
) -> Result<Option<TestsSummary>> {
    let config = TestRunConfig {
        filter,
        ignored,
        include_ignored,
        profiler_config: None,
        gas_enabled: !gas_disabled,
        print_resource_usage,
    };

    let runner = TestRunner::new_with_virtual_project(
        project_name,
        files,
        starknet,
        allow_warnings,
        config,
    )?;
    runner.run()
}

#[allow(clippy::too_many_arguments)]
pub fn run_tests_with_virtual_project_parsed(
    project_name: &str,
    files: &HashMap<String, String>,
    allow_warnings: bool,
    filter: String,
    include_ignored: bool,
    ignored: bool,
    starknet: bool,
    gas_disabled: bool,
    print_resource_usage: bool,
) -> Result<String> {
    let result = run_tests_with_virtual_project(
        project_name,
        files,
        allow_warnings,
        filter,
        include_ignored,
        ignored,
        starknet,
        gas_disabled,
        print_resource_usage,
    );
    format_test_result(result)
}

// ========== Multi-file test execution with dependencies (new) ==========

#[allow(clippy::too_many_arguments)]
pub fn run_tests_with_virtual_project_and_deps(
    project_name: &str,
    files: &HashMap<String, String>,
    dependencies: &HashMap<String, DependencyInput>,
    allow_warnings: bool,
    filter: String,
    include_ignored: bool,
    ignored: bool,
    starknet: bool,
    gas_disabled: bool,
    print_resource_usage: bool,
) -> Result<Option<TestsSummary>> {
    let config = TestRunConfig {
        filter,
        ignored,
        include_ignored,
        profiler_config: None,
        gas_enabled: !gas_disabled,
        print_resource_usage,
    };

    let runner = TestRunner::new_with_virtual_project_and_deps(
        project_name,
        files,
        dependencies,
        starknet,
        allow_warnings,
        config,
    )?;
    runner.run()
}

#[allow(clippy::too_many_arguments)]
pub fn run_tests_with_virtual_project_and_deps_parsed(
    project_name: &str,
    files: &HashMap<String, String>,
    dependencies: &HashMap<String, DependencyInput>,
    allow_warnings: bool,
    filter: String,
    include_ignored: bool,
    ignored: bool,
    starknet: bool,
    gas_disabled: bool,
    print_resource_usage: bool,
) -> Result<String> {
    let result = run_tests_with_virtual_project_and_deps(
        project_name,
        files,
        dependencies,
        allow_warnings,
        filter,
        include_ignored,
        ignored,
        starknet,
        gas_disabled,
        print_resource_usage,
    );
    format_test_result(result)
}

// ========== Shared helpers ==========

fn format_test_result(result: Result<Option<TestsSummary>>) -> Result<String> {
    match result {
        Ok(Some(tests_summary)) => {
            let msg = format!(
                "test result: passed: {:?}, failed: {:?}, ignored: {:?}",
                tests_summary.passed.len(),
                tests_summary.failed.len(),
                tests_summary.ignored.len()
            );
            Ok(msg)
        }
        Ok(None) => Ok("All tests passed.".to_string()),
        Err(e) => Err(e),
    }
}
