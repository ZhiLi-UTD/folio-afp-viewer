//! Writes the synthetic AFP fixtures to `tools/afpgen/out/`.
//!
//! Run from the repo root: `cargo run -p afpgen`. The output directory is
//! git-ignored; fixtures are reproducible from this generator.

use std::fs;
use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    fs::create_dir_all(&out)?;
    for (name, bytes) in afpgen::all_fixtures() {
        let path = out.join(name);
        fs::write(&path, &bytes)?;
        println!("wrote {} ({} bytes)", path.display(), bytes.len());
    }
    Ok(())
}
