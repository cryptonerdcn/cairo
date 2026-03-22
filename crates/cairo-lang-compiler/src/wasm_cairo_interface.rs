use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use cairo_lang_defs::db::DefsGroup;
use cairo_lang_defs::ids::ModuleId;
use cairo_lang_filesystem::db::{
    CORELIB_VERSION, CrateConfiguration, CrateSettings, DependencySettings, Edition,
    ExperimentalFeaturesConfig, FilesGroup, set_crate_configs_input,
    update_crate_configuration_input_helper,
};
use cairo_lang_filesystem::ids::{
    CrateId, CrateInput, CrateLongId, Directory, FileId, FileKind, FileLongId, SmolStrId,
    VirtualFile,
};
use cairo_lang_filesystem::{override_file_content, set_crate_config};
use cairo_lang_sierra::program::Program;
use cairo_lang_utils::Intern;
use include_dir::{Dir, include_dir};
use salsa::Database;

use crate::db::RootDatabase;
use crate::diagnostics::DiagnosticsReporter;
use crate::project::ProjectError;
use crate::{CompilerConfig, compile_prepared_db_program};

/// Input for a dependency crate passed from the WASM/JS layer.
pub struct DependencyInput {
    /// Map of relative file paths to their content (e.g. "lib.cairo" -> "mod erc20;").
    pub files: HashMap<String, String>,
    /// The Cairo edition for this dependency (e.g. "2024_07"). Defaults to Edition::default() if
    /// None.
    pub edition: Option<String>,
    /// This dependency's own dependencies (names only, must also be in the top-level deps map).
    pub dependencies: Vec<String>,
}

/// Parse an edition string like "2023_01", "2023_10", "2023_11", "2024_07", "2025_12"
/// into the corresponding `Edition` enum variant. Returns `Edition::default()` on unknown input.
fn parse_edition(s: &str) -> Edition {
    match s {
        "2023_01" => Edition::V2023_01,
        "2023_10" => Edition::V2023_10,
        "2023_11" => Edition::V2023_11,
        "2024_07" => Edition::V2024_07,
        "2025_12" => Edition::V2025_12,
        _ => Edition::default(),
    }
}

// Embed the entire corelib/src directory at compile time
static EMBEDDED_CORELIB: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../corelib/src");

/// Initialize corelib in the database using embedded files.
/// This works in both native and WASM environments, avoiding filesystem-based detection.
pub fn init_corelib(db: &mut RootDatabase) {
    let core = CrateLongId::core(db).intern(db);
    let virtual_dir = build_corelib_virtual_directory(db, &EMBEDDED_CORELIB);
    let root = CrateConfiguration {
        root: virtual_dir,
        settings: CrateSettings {
            name: None,
            edition: Edition::V2025_12,
            version: semver::Version::parse(CORELIB_VERSION).ok(),
            cfg_set: Default::default(),
            dependencies: Default::default(),
            experimental_features: ExperimentalFeaturesConfig {
                negative_impls: true,
                associated_item_constraints: true,
                coupons: true,
                user_defined_inline_macros: true,
                repr_ptrs: true,
            },
        },
        cache_file: None,
    };
    let crate_configs = update_crate_configuration_input_helper(db, core, Some(root));
    set_crate_configs_input(db, Some(crate_configs));
}

/// Build a `Directory::Virtual` tree from an embedded `include_dir::Dir`.
fn build_corelib_virtual_directory<'db>(db: &'db dyn Database, dir: &Dir<'_>) -> Directory<'db> {
    let mut files: BTreeMap<String, FileId<'db>> = BTreeMap::new();
    let mut dirs: BTreeMap<String, Box<Directory<'db>>> = BTreeMap::new();

    for file in dir.files() {
        let name = file.path().file_name().unwrap().to_str().unwrap().to_string();
        let content = std::str::from_utf8(file.contents()).unwrap();
        let file_id = create_virtual_file(db, &name, content);
        files.insert(name, file_id);
    }

    for subdir in dir.dirs() {
        let name = subdir.path().file_name().unwrap().to_str().unwrap().to_string();
        let sub_virtual = build_corelib_virtual_directory(db, subdir);
        dirs.insert(name, Box::new(sub_virtual));
    }

    Directory::Virtual { files, dirs }
}

// ========== Single-file compilation (existing) ==========

/// Compiles a Cairo project with input string.
pub fn compile_cairo_project_with_input_string(
    path: &Path,
    input: &str,
    compiler_config: CompilerConfig<'_>,
) -> Result<Program> {
    let mut db = RootDatabase::builder().build()?;
    init_corelib(&mut db);
    let main_crate_ids = setup_project_with_input_string(&mut db, path, input)?;
    let crate_ids = CrateInput::into_crate_ids(&db, main_crate_ids.clone());

    check_diagnostics(&db, &main_crate_ids)?;
    compile_prepared_db_program(&db, crate_ids, compiler_config)
}

/// Setup the 'db' to compile the project in the given string.
pub fn setup_project_with_input_string(
    db: &mut RootDatabase,
    path: &Path,
    input: &str,
) -> Result<Vec<CrateInput>, ProjectError> {
    Ok(vec![setup_single_file_project_with_input_string(db, path, input)?])
}

/// Setup to 'db' to compile single file with input string.
pub fn setup_single_file_project_with_input_string(
    db: &mut RootDatabase,
    path: &Path,
    input: &str,
) -> Result<CrateInput, ProjectError> {
    let file_stem = "astro";

    let crate_id = CrateId::plain(db, SmolStrId::from(db, file_stem));
    set_crate_config!(
        db,
        crate_id,
        Some(CrateConfiguration::default_for_root(Directory::Real(
            path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf()
        )))
    );

    let crate_id = CrateId::plain(db, SmolStrId::from(db, file_stem));
    let module_id = ModuleId::CrateRoot(crate_id);
    let file_id = db.module_main_file(module_id).unwrap();
    override_file_content!(db, file_id, Some(Arc::from(input)));

    let crate_id = CrateId::plain(db, SmolStrId::from(db, file_stem));
    Ok(crate_id.long(db).clone().into_crate_input(db))
}

// ========== Multi-file project compilation (new) ==========

/// Compiles a Cairo project with multiple virtual files.
///
/// # Arguments
/// * `project_name` - The name of the project/crate.
/// * `files` - A map of relative file paths to their content (e.g. "lib.cairo" -> "mod foo;").
/// * `compiler_config` - The compiler configuration.
pub fn compile_cairo_project_with_virtual_files(
    project_name: &str,
    files: &HashMap<String, String>,
    compiler_config: CompilerConfig<'_>,
) -> Result<Program> {
    let mut db = RootDatabase::builder().build()?;
    init_corelib(&mut db);
    let main_crate_ids = setup_virtual_project(&mut db, project_name, files);
    let crate_ids = CrateInput::into_crate_ids(&db, main_crate_ids.clone());

    check_diagnostics(&db, &main_crate_ids)?;
    compile_prepared_db_program(&db, crate_ids, compiler_config)
}

/// Compiles a Cairo project with multiple virtual files and external dependencies.
///
/// # Arguments
/// * `project_name` - The name of the project/crate.
/// * `files` - A map of relative file paths to their content (e.g. "lib.cairo" -> "mod foo;").
/// * `dependencies` - A map of dependency name to DependencyInput.
/// * `compiler_config` - The compiler configuration.
pub fn compile_cairo_project_with_virtual_files_and_deps(
    project_name: &str,
    files: &HashMap<String, String>,
    dependencies: &HashMap<String, DependencyInput>,
    compiler_config: CompilerConfig<'_>,
) -> Result<Program> {
    let mut db = RootDatabase::builder().build()?;
    init_corelib(&mut db);
    let main_crate_ids =
        setup_virtual_project_with_deps(&mut db, project_name, files, dependencies);
    let crate_ids = CrateInput::into_crate_ids(&db, main_crate_ids.clone());

    check_diagnostics(&db, &main_crate_ids)?;
    compile_prepared_db_program(&db, crate_ids, compiler_config)
}

/// Setup the 'db' with a virtual multi-file project.
/// Files should be keyed by their path relative to `src/` (e.g. "lib.cairo", "utils.cairo",
/// "contract/lib.cairo"). If paths start with "src/", the prefix is stripped automatically.
pub fn setup_virtual_project(
    db: &mut RootDatabase,
    project_name: &str,
    files: &HashMap<String, String>,
) -> Vec<CrateInput> {
    let root_dir = build_virtual_directory(db, files);

    let crate_id = CrateId::plain(db, SmolStrId::from(db, project_name));
    set_crate_config!(db, crate_id, Some(CrateConfiguration::default_for_root(root_dir)));

    let crate_id = CrateId::plain(db, SmolStrId::from(db, project_name));
    vec![crate_id.long(db).clone().into_crate_input(db)]
}

/// Setup the 'db' with a virtual multi-file project and its dependencies.
///
/// Each entry in `dependencies` maps a dependency name (e.g. "openzeppelin_token") to its
/// `DependencyInput`, which contains the dependency's source files, edition, and its own
/// transitive dependency names.
///
/// The main project crate will have all top-level dependencies registered, and each dependency
/// crate will have its own sub-dependencies registered.
pub fn setup_virtual_project_with_deps(
    db: &mut RootDatabase,
    project_name: &str,
    files: &HashMap<String, String>,
    dependencies: &HashMap<String, DependencyInput>,
) -> Vec<CrateInput> {
    // 1. Register each dependency crate.
    for (dep_name, dep_input) in dependencies {
        let dep_root_dir = build_virtual_directory(db, &dep_input.files);

        let edition = dep_input.edition.as_deref().map(parse_edition).unwrap_or(Edition::V2024_07);

        // Build the dependency's own dependencies mapping.
        let mut dep_dependencies = BTreeMap::new();
        for sub_dep_name in &dep_input.dependencies {
            dep_dependencies
                .insert(sub_dep_name.clone(), DependencySettings { discriminator: None });
        }

        let dep_crate_id = CrateId::plain(db, SmolStrId::from(db, dep_name.as_str()));
        let dep_config = CrateConfiguration {
            root: dep_root_dir,
            settings: CrateSettings {
                name: None,
                edition,
                version: None,
                cfg_set: Default::default(),
                dependencies: dep_dependencies,
                experimental_features: ExperimentalFeaturesConfig::default(),
            },
            cache_file: None,
        };
        set_crate_config!(db, dep_crate_id, Some(dep_config));
    }

    // 2. Register the main project crate with dependencies on all top-level deps.
    let root_dir = build_virtual_directory(db, files);

    let mut main_dependencies = BTreeMap::new();
    for dep_name in dependencies.keys() {
        main_dependencies.insert(dep_name.clone(), DependencySettings { discriminator: None });
    }

    let crate_id = CrateId::plain(db, SmolStrId::from(db, project_name));
    let main_config = CrateConfiguration {
        root: root_dir,
        settings: CrateSettings {
            name: None,
            edition: Edition::V2024_07,
            version: None,
            cfg_set: Default::default(),
            dependencies: main_dependencies,
            experimental_features: ExperimentalFeaturesConfig::default(),
        },
        cache_file: None,
    };
    set_crate_config!(db, crate_id, Some(main_config));

    let crate_id = CrateId::plain(db, SmolStrId::from(db, project_name));
    vec![crate_id.long(db).clone().into_crate_input(db)]
}

/// Build a `Directory::Virtual` tree from a flat file map.
fn build_virtual_directory<'db>(
    db: &'db dyn Database,
    files: &HashMap<String, String>,
) -> Directory<'db> {
    let mut root_files: BTreeMap<String, FileId<'db>> = BTreeMap::new();
    let mut subdirs: HashMap<String, HashMap<String, String>> = HashMap::new();

    for (path, content) in files {
        // Strip "src/" prefix if present
        let path = path.strip_prefix("src/").unwrap_or(path);

        let parts: Vec<&str> = path.splitn(2, '/').collect();
        if parts.len() == 1 {
            let file_id = create_virtual_file(db, parts[0], content);
            root_files.insert(parts[0].to_string(), file_id);
        } else {
            subdirs
                .entry(parts[0].to_string())
                .or_default()
                .insert(parts[1].to_string(), content.clone());
        }
    }

    let mut root_dirs: BTreeMap<String, Box<Directory<'db>>> = BTreeMap::new();
    for (dir_name, sub_files) in subdirs {
        let sub_dir = build_virtual_directory(db, &sub_files);
        root_dirs.insert(dir_name, Box::new(sub_dir));
    }

    Directory::Virtual { files: root_files, dirs: root_dirs }
}

/// Create a virtual file and intern it into the database.
fn create_virtual_file<'db>(db: &'db dyn Database, name: &str, content: &str) -> FileId<'db> {
    FileLongId::Virtual(VirtualFile {
        parent: None,
        name: SmolStrId::from(db, name),
        content: SmolStrId::from(db, content),
        code_mappings: [].into(),
        kind: FileKind::Module,
        original_item_removed: false,
    })
    .intern(db)
}

// ========== Shared helpers ==========

/// Check diagnostics and bail with error string if any found.
fn check_diagnostics(db: &RootDatabase, crate_ids: &[CrateInput]) -> Result<()> {
    let mut diagnostics = String::new();
    let mut reporter =
        DiagnosticsReporter::write_to_string(&mut diagnostics).with_crates(crate_ids);
    let found = reporter.check(db);
    drop(reporter);
    if found {
        anyhow::bail!("failed to compile:\n {}", diagnostics);
    }
    Ok(())
}
