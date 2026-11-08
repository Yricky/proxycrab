use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

/// Prepare the runtime Skill without development tests or evaluation fixtures.
pub fn prepare(target: &Path) -> io::Result<()> {
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../skills/proxycrab");
    println!("cargo:rerun-if-changed={}", source.display());
    if target.exists() {
        fs::remove_dir_all(target)?;
    }
    fs::create_dir_all(target)?;
    fs::copy(source.join("SKILL.md"), target.join("SKILL.md"))?;
    for directory in ["references", "scripts"] {
        copy_directory(&source.join(directory), &target.join(directory))?;
    }
    Ok(())
}

fn copy_directory(source: &Path, target: &Path) -> io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_directory(&entry.path(), &target.join(name))?;
        } else if file_type.is_file() && !name.to_string_lossy().ends_with(".test.mjs") {
            fs::copy(entry.path(), target.join(name))?;
        }
    }
    Ok(())
}
