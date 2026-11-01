//! Builds Giac into the app when `GIAC_WORKER` names the worker .exe (the
//! Windows CI build does). Without it the app works the same, with Solve
//! using only its own algebra.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(giac)");
    println!("cargo::rerun-if-env-changed=GIAC_WORKER");
    if let Some(path) = std::env::var_os("GIAC_WORKER") {
        let path = std::path::PathBuf::from(path);
        if path.is_file() {
            println!("cargo::rerun-if-changed={}", path.display());
            println!("cargo::rustc-cfg=giac");
            println!("cargo::rustc-env=GIAC_WORKER_PATH={}", path.display());
        } else {
            println!(
                "cargo::warning=GIAC_WORKER is set but {} doesn't exist; building without Giac",
                path.display()
            );
        }
    }
}
