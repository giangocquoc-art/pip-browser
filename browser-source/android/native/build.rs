// Adapted from the existing Windows build.rs domain table generator.
// Canonical text + u32 offsets are generated on CI, never sorted/allocated at app startup.
use std::{collections::BTreeSet, env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest path"))
        .join("../../crates/app/filters");
    let gambling_path = root.join("gambling-hagezi-mini-domains.txt");
    let popup_path = root.join("popup-overrides.txt");
    for path in [&gambling_path, &popup_path] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let gambling = fs::read_to_string(gambling_path).expect("bundled gambling domains");
    let popup = fs::read_to_string(popup_path).expect("bundled popup overrides");
    let mut domains: BTreeSet<&str> = gambling
        .lines()
        .map(str::trim)
        .filter(|host| !host.is_empty() && !host.starts_with('#'))
        .collect();
    domains.extend(
        popup
            .lines()
            .filter_map(|rule| rule.trim().strip_prefix("||")?.strip_suffix("^$popup")),
    );
    let mut canonical = String::new();
    let mut offsets = Vec::with_capacity(domains.len());
    for domain in domains {
        assert!(
            domain.len() <= 253
                && domain.contains('.')
                && domain.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'-'
                    || b == b'.')
                && domain.split('.').all(|label| !label.is_empty()),
            "invalid bundled domain: {domain}"
        );
        offsets.push(u32::try_from(canonical.len()).expect("domain table below 4 GiB"));
        canonical.push_str(domain);
        canonical.push('\n');
    }
    let mut table = format!(
        "static NAVIGATION_DOMAIN_OFFSETS: [u32; {}] = [\n",
        offsets.len()
    );
    for chunk in offsets.chunks(12) {
        for offset in chunk {
            table.push_str(&format!("{offset},"));
        }
        table.push('\n');
    }
    table.push_str("];\n");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output path"));
    fs::write(output.join("navigation-domains.txt"), canonical).expect("write canonical domains");
    fs::write(output.join("navigation-offsets.rs"), table).expect("write static offsets");
}
