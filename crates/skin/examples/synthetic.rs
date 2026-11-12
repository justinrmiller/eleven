//! Writes a placeholder `.wsz` skin (flat colour per sheet) for trying out skin support.
//!
//! Usage: `cargo run -p eleven-skin --example synthetic -- out.wsz`

use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let out = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("synthetic.wsz"), PathBuf::from);
    std::fs::write(
        &out,
        eleven_skin::synthetic::SyntheticSkin::default().build(),
    )?;
    println!("wrote {}", out.display());
    Ok(())
}
