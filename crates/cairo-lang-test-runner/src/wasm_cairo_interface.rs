use std::f64::consts::E;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Result};

use cairo_lang_compiler::db::RootDatabase;
use cairo_lang_compiler::diagnostics::DiagnosticsReporter;
use cairo_lang_compiler::wasm_cairo_interface::setup_project_with_input_string;
use cairo_lang_filesystem::cfg::{Cfg, CfgSet};
use cairo_lang_filesystem::db::FilesGroupEx;
use cairo_lang_filesystem::flag::Flag;
use cairo_lang_filesystem::ids::FlagId;
use cairo_lang_filesystem::log_db::LogDatabase;

use cairo_lang_starknet::starknet_plugin_suite;
use cairo_lang_test_plugin::{test_plugin_suite, TestsCompilationConfig};

use crate::{RunProfilerConfig, TestCompiler, TestRunConfig, TestRunner, TestsSummary};

impl TestRunner {
    /// Configure a new test runner
    ///
    /// # Arguments
    ///
    /// * `path` - The path to compile and run its tests
    /// * `filter` - Run only tests containing the filter string
    /// * `include_ignored` - Include ignored tests as well
    /// * `ignored` - Run ignored tests only
    /// * `starknet` - Add the starknet plugin to run the tests
    pub fn new_with_string(
        input_program_string: &String,
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
                add_statements_functions: config.run_profiler == RunProfilerConfig::Cairo,
            },
        )?;
        Ok(Self { compiler, config })
    }
}

impl TestCompiler {
    /// Configure a new test compiler
    ///
    /// # Arguments
    ///
    /// * `path` - The path to compile and run its tests
    /// * `starknet` - Add the starknet plugin to run the tests
    pub fn try_new_with_string(
        input_program_string: &String,
        path: &Path,
        allow_warnings: bool,
        gas_enabled: bool,
        config: TestsCompilationConfig,
    ) -> Result<Self> {
        let db = &mut {
            let mut b = RootDatabase::builder();
            if !gas_enabled {
                b.skip_auto_withdraw_gas();
            }
            b.detect_corelib();
            b.with_cfg(CfgSet::from_iter([Cfg::name("test"), Cfg::kv("target", "test")]));
            b.with_plugin_suite(test_plugin_suite());
            if config.starknet {
                b.with_plugin_suite(starknet_plugin_suite());
            }
            b.build()?
        };
        let add_redeposit_gas_flag_id = FlagId::new(db, "add_redeposit_gas");
        db.set_flag(add_redeposit_gas_flag_id, Some(Arc::new(Flag::AddRedepositGas(true))));
        // WASM-Cairo interface
        let main_crate_ids =
            setup_project_with_input_string(db, Path::new(&path), input_program_string)?;
        
        Ok(Self {
            db: db.snapshot(),
            test_crate_ids: main_crate_ids.clone(),
            main_crate_ids,
            allow_warnings,
            config,
        })
    }
}

pub fn run_tests_with_input_string(
    input_program_string: &String,
    allow_warnings: bool,
    filter: String,
    include_ignored: bool,
    ignored: bool,
    starknet: bool,
    run_profiler: String,
    gas_disabled: bool,
    print_resource_usage: bool,
) -> Result<Option<TestsSummary>> {
    let path = Path::new("astro");
    let config = TestRunConfig {
        filter,
        ignored,
        include_ignored,
        run_profiler: RunProfilerConfig::None, // TODO: parse run_profiler
        gas_enabled: !gas_disabled,
        print_resource_usage,
    };

    let runner = TestRunner::new_with_string(
        &input_program_string,
        path,
        starknet,
        allow_warnings,
        config,
    )?;
    let result = runner.run();
    result
}

pub fn run_tests_with_input_string_parsed(
    input_program_string: &String,
    allow_warnings: bool,
    filter: String,
    include_ignored: bool,
    ignored: bool,
    starknet: bool,
    run_profiler: String,
    gas_disabled: bool,
    print_resource_usage: bool,
) -> Result<String> {
    let result = run_tests_with_input_string(&input_program_string, allow_warnings, filter, include_ignored, ignored, starknet, String::new(), gas_disabled, print_resource_usage);
    match result {
        Ok( Some(tests_summary) ) => {
            println!("Test result passed: {:?}", tests_summary.passed);
            println!("Test result failed: {:?}", tests_summary.failed);
            println!("Test result ignored: {:?}", tests_summary.ignored);
            return Ok(LogDatabase::get_file_text("test_log_file".to_string()));
        },
        Ok( None ) => {
            println!("No tests summary found.");
            return Ok(LogDatabase::get_file_text("test_log_file".to_string()));
        },
        Err(e) => Err(e)
    }
}