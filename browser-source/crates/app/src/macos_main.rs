//! Native macOS preview. The Windows binary remains the default Cargo target.

#[path = "macos/navigation.rs"]
mod navigation;

#[cfg(target_os = "macos")]
#[path = "macos/shell.rs"]
mod shell;

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    shell::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("pip-macos requires macOS. Use the default Pip binary on Windows.");
    std::process::exit(2);
}
