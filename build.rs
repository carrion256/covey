use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let migrations_dir = Path::new(&manifest_dir).join("src/migrations");

    println!("cargo:rerun-if-changed={}", migrations_dir.display());

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
