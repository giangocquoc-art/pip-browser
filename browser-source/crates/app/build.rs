fn main() {
    generate_gambling_domain_table();

    #[cfg(windows)]
    compile_windows_resources();
}

fn generate_gambling_domain_table() {
    use std::{collections::BTreeSet, fs, path::PathBuf};

    let source_path = PathBuf::from("filters/gambling-hagezi-mini-domains.txt");
    println!("cargo:rerun-if-changed={}", source_path.display());
    let source = fs::read_to_string(&source_path).unwrap_or_else(|error| {
        panic!(
            "failed to read bundled gambling domains {}: {error}",
            source_path.display()
        )
    });

    let domains: BTreeSet<&str> = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let mut canonical = String::new();
    let mut offsets = Vec::with_capacity(domains.len());
    for domain in domains {
        assert!(domain.is_ascii(), "gambling domain must be ASCII: {domain}");
        offsets.push(u32::try_from(canonical.len()).expect("domain source must be under 4 GiB"));
        canonical.push_str(domain);
        canonical.push('\n');
    }

    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    fs::write(out_dir.join("gambling-domains.txt"), canonical)
        .expect("failed to write canonical gambling domain source");

    let mut table = format!(
        "pub static GAMBLING_DOMAIN_OFFSETS: [u32; {}] = [\n",
        offsets.len()
    );
    for chunk in offsets.chunks(12) {
        table.push_str("    ");
        for offset in chunk {
            table.push_str(&format!("{offset}, "));
        }
        table.push('\n');
    }
    table.push_str("];\n");
    fs::write(out_dir.join("gambling-domain-offsets.rs"), table)
        .expect("failed to write gambling domain offset table");
}

#[cfg(windows)]
fn compile_windows_resources() {
    let mut res = winres::WindowsResource::new();
    res.set("ProductName", "Pip");
    res.set("FileDescription", "Pip Browser");
    res.set_icon("ui/icon.ico");
    if let Some(sdk_bin) = find_windows_sdk_bin() {
        res.set_toolkit_path(&sdk_bin);
    }
    if let Err(e) = res.compile() {
        eprintln!("cargo:warning=Failed to compile Windows resource: {}", e);
    }
}

#[cfg(windows)]
fn find_windows_sdk_bin() -> Option<String> {
    let prog_files = std::env::var("ProgramFiles(x86)")
        .or_else(|_| std::env::var("ProgramFiles"))
        .unwrap_or_else(|_| r"C:\Program Files (x86)".to_string());
    let base = std::path::PathBuf::from(prog_files).join(r"Windows Kits\10\bin");
    if let Ok(entries) = std::fs::read_dir(&base) {
        let mut sdk_dirs: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir() && e.path().join("x64").join("rc.exe").exists())
            .map(|e| e.path().join("x64"))
            .collect();
        sdk_dirs.sort();
        if let Some(highest) = sdk_dirs.pop() {
            return Some(highest.to_string_lossy().to_string());
        }
    }
    None
}
