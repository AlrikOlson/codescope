/// Auto-detect CUDA toolkit at build time.
///
/// When the `cuda` feature is enabled, cudarc needs `nvcc` on PATH.
/// This build script finds CUDA even when it's not on PATH and sets
/// CUDA_ROOT so downstream crates (cudarc/bindgen_cuda) can find it.
///
/// For users: just `cargo install --features cuda` — no PATH fiddling needed.
fn main() {
    // Only relevant when building with cuda feature
    if std::env::var("CARGO_FEATURE_CUDA").is_err() {
        return;
    }

    // If nvcc is already on PATH, nothing to do
    let nvcc_on_path = std::process::Command::new("nvcc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if nvcc_on_path {
        return;
    }

    // Probe common CUDA install locations
    let search_paths = [
        "/usr/local/cuda",
        "/usr/local/cuda-13",
        "/usr/local/cuda-13.1",
        "/usr/local/cuda-12",
        "/usr/local/cuda-12.6",
        "/usr/local/cuda-12.4",
        "/usr/local/cuda-12.2",
        "/usr/local/cuda-11",
        "/opt/cuda",
    ];

    for base in &search_paths {
        let nvcc = format!("{}/bin/nvcc", base);
        if std::path::Path::new(&nvcc).exists() {
            // Tell downstream build scripts where CUDA lives
            println!("cargo:rustc-env=CUDA_ROOT={}", base);
            // Also inject into PATH for this build's child processes
            let current_path = std::env::var("PATH").unwrap_or_default();
            std::env::set_var("PATH", format!("{}/bin:{}", base, current_path));
            println!("cargo:warning=Auto-detected CUDA at {}", base);
            return;
        }
    }

    // Windows: CUDA_PATH env var
    if let Ok(cuda_path) = std::env::var("CUDA_PATH") {
        let nvcc = std::path::Path::new(&cuda_path).join("bin").join("nvcc");
        if nvcc.exists() || nvcc.with_extension("exe").exists() {
            println!("cargo:rustc-env=CUDA_ROOT={}", cuda_path);
            return;
        }
    }

    println!("cargo:warning=cuda feature enabled but nvcc not found — build may fail");
}
