use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let samples_dir = manifest_dir.join("scenarios").join("samples");

    println!("cargo:rerun-if-changed={}", samples_dir.display());

    let mut entries = fs::read_dir(&samples_dir)
        .unwrap_or_else(|err| panic!("reading {}: {err}", samples_dir.display()))
        .map(|entry| entry.expect("reading sample directory entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|path| {
            println!("cargo:rerun-if-changed={}", path.display());
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("sample filename must be valid UTF-8")
                .to_owned();
            (name, path)
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut registry = String::from("&[\n");
    for (name, path) in entries {
        let path = path.display().to_string();
        registry.push_str(&format!("    ({name:?}, include_str!({path:?})),\n"));
    }
    registry.push_str("]\n");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out_dir.join("sample_registry.rs"), registry).expect("writing sample registry");
}
