//! Gives the Windows program its icon (see `build/windows_icon.rs`).

include!("build/windows_icon.rs");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/windows_icon.rs");
    link_windows_icon("assets/linewright.ico");
}
