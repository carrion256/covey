use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let manifest_path = Path::new(&manifest_dir);
    let migrations_dir = manifest_path.join("src/migrations");

    println!("cargo:rerun-if-changed={}", migrations_dir.display());
    for compiled_input in ["src", "Cargo.toml", "Cargo.lock", "build.rs"] {
        println!(
            "cargo:rerun-if-changed={}",
            manifest_path.join(compiled_input).display()
        );
    }

    println!(
        "cargo:rustc-env=COVEY_BUILD_COMMIT={}",
        build_commit(&manifest_dir)
    );
    println!(
        "cargo:rustc-env=COVEY_BUILD_DIRTY={}",
        build_dirty(&manifest_dir)
    );

    let mut migrations = migration_files(&migrations_dir);
    migrations.sort_by_key(|migration| migration.number);
    assert!(!migrations.is_empty(), "at least one migration is required");
    assert_unique_numbers(&migrations);
    for migration in &migrations {
        println!(
            "cargo:rerun-if-changed={}",
            migrations_dir.join(&migration.file_name).display()
        );
    }

    let generated = render_migrations(&migrations);
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    fs::write(out_dir.join("covey_migrations.rs"), generated).expect("write generated migrations");
}

fn git_output(manifest_dir: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(manifest_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Returns the 12-character short hash of HEAD, or `unknown` when git or the
/// repository is unavailable.
fn build_commit(manifest_dir: &str) -> String {
    let short: String = git_output(manifest_dir, &["rev-parse", "HEAD"])
        .unwrap_or_default()
        .trim()
        .chars()
        .take(12)
        .collect();
    if short.len() == 12 {
        short
    } else {
        "unknown".to_owned()
    }
}

/// Returns `true` when tracked files differ from HEAD, `false` when they do
/// not or when git is unavailable.
fn build_dirty(manifest_dir: &str) -> bool {
    match git_output(
        manifest_dir,
        &["status", "--porcelain", "--untracked-files=no"],
    ) {
        Some(status) => !status.trim().is_empty(),
        None => false,
    }
}

#[derive(Debug)]
struct MigrationFile {
    number: u64,
    file_name: String,
}

fn migration_files(migrations_dir: &Path) -> Vec<MigrationFile> {
    fs::read_dir(migrations_dir)
        .unwrap_or_else(|error| {
            panic!(
                "read migration directory {}: {error}",
                migrations_dir.display()
            )
        })
        .map(|entry| {
            let entry = entry.expect("read migration directory entry");
            let file_name = entry
                .file_name()
                .into_string()
                .unwrap_or_else(|_| panic!("migration file name must be valid utf-8"));
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("sql") {
                panic!("migration entries must be .sql files: {}", path.display());
            }
            MigrationFile {
                number: migration_number(&file_name),
                file_name,
            }
        })
        .collect()
}

fn migration_number(file_name: &str) -> u64 {
    let digits: String = file_name
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        panic!("migration file must start with an order number: {file_name}");
    }
    digits
        .parse()
        .unwrap_or_else(|_| panic!("migration number is valid: {file_name}"))
}

fn assert_unique_numbers(migrations: &[MigrationFile]) {
    for pair in migrations.windows(2) {
        if pair[0].number == pair[1].number {
            panic!(
                "duplicate migration number {} in {} and {}",
                pair[0].number, pair[0].file_name, pair[1].file_name
            );
        }
    }
}

fn render_migrations(migrations: &[MigrationFile]) -> String {
    let mut output = String::from("pub(crate) const MIGRATIONS: &[&str] = &[\n");
    for migration in migrations {
        output
            .push_str("    include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/migrations/");
        output.push_str(&migration.file_name);
        output.push_str("\")),\n");
    }
    output.push_str("];\n");
    output
}
