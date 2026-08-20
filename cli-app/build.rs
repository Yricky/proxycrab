use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn collect(root: &Path, directory: &Path, files: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, files);
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, path));
        }
    }
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("..").join("dist").join("cli");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    collect(&root, &root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    if files.is_empty() {
        println!(
            "cargo:warning=CLI UI assets are absent; run `pnpm build:cli` before a release build"
        );
    }
    let entries = files
        .into_iter()
        .map(|(relative, path)| {
            format!("({relative:?}, include_bytes!({path:?}) as &'static [u8])")
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let output = format!("pub static UI_ASSETS: &[(&str, &[u8])] = &[{entries}];\n");
    let destination = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("ui_assets.rs");
    fs::write(destination, output).unwrap();
}
