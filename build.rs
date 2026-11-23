use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=assets/AutumnDeck.res");
    let res_path = Path::new("assets/AutumnDeck.res");
    if res_path.exists() {
        if let Ok(abs) = std::fs::canonicalize(res_path) {
            // Strip \\?\ extended prefix so MSVC link.exe accepts the path cleanly
            let s = abs.to_string_lossy();
            let clean = s.strip_prefix(r"\\?\").unwrap_or(&s);
            println!("cargo:rustc-link-arg-bin=AutumnDeck={}", clean);
        }
    }
}
