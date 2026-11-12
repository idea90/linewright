//! Puts the Linewright program inside the setup, compressed, and gives the setup the program's icon on Windows.
//!
//! The program comes from the file LINEWRIGHT_PAYLOAD names, built first with `cargo build --release`:
//!
//! ```sh
//! cargo build --release
//! LINEWRIGHT_PAYLOAD=target/release/linewright cargo build --release -p linewright-setup
//! ```
//!
//! Without it the setup is built with nothing inside, which is enough to work on its window, and says so when asked
//! to install.

use std::io::Write;

include!("../build/windows_icon.rs");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../build/windows_icon.rs");
    println!("cargo:rerun-if-env-changed=LINEWRIGHT_PAYLOAD");
    link_windows_icon("../assets/linewright.ico");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("payload.gz");
    let compressed = match std::env::var_os("LINEWRIGHT_PAYLOAD") {
        Some(path) => {
            let path = std::path::PathBuf::from(path);
            // A relative path is from the workspace, where the command is usually typed.
            let path = if path.is_relative() { std::path::Path::new("..").join(path) } else { path };
            println!("cargo:rerun-if-changed={}", path.display());
            let program = std::fs::read(&path).unwrap_or_else(|e| panic!("LINEWRIGHT_PAYLOAD {}: {e}", path.display()));
            let mut encoder = flate2::write::GzEncoder::new(vec![], flate2::Compression::best());
            encoder.write_all(&program).expect("compressing the program");
            encoder.finish().expect("compressing the program")
        }
        None => vec![],
    };
    std::fs::write(out, compressed).expect("the payload");
}
