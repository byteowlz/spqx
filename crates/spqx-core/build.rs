// Build script for qwen3-tts-rs.
//
// When the `mlx` feature is enabled (macOS only), this script:
// 1. Builds the mlx-c library from the git submodule via CMake
// 2. Emits linker directives for mlx-c, MLX, Metal, and system frameworks

fn main() {
    #[cfg(feature = "mlx")]
    build_mlx();
}

#[cfg(feature = "mlx")]
fn build_mlx() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    if target_os != "macos" {
        panic!("The `mlx` feature is only supported on macOS. Current target OS: {target_os}");
    }
    if target_arch != "aarch64" {
        eprintln!(
            "Warning: MLX is optimized for Apple Silicon (aarch64). \
             Current target arch: {target_arch}. Metal GPU acceleration may not be available."
        );
    }

    let mlx_c_dir = std::path::PathBuf::from("../../mlx-c");
    if !mlx_c_dir.join("CMakeLists.txt").exists() {
        panic!(
            "mlx-c submodule not found. Please run:\n\
             \n\
             git submodule update --init --recursive\n\
             \n\
             to clone the mlx-c dependency."
        );
    }

    // Build mlx-c via CMake
    let build_mlx_c = || {
        cmake::Config::new(&mlx_c_dir)
            .define("MLX_BUILD_TESTS", "OFF")
            .define("MLX_BUILD_EXAMPLES", "OFF")
            .define("MLX_BUILD_BENCHMARKS", "OFF")
            .define("BUILD_SHARED_LIBS", "OFF")
            // An explicit deployment target keeps clang's __builtin_available
            // machinery sane under the Xcode 26 toolchain on macOS 15. Left
            // empty, MLX's availability-gated kernel dispatch mis-resolves
            // (the metal4.0 JIT misfire is one symptom) and can silently
            // select slow fallback kernels — measured 4-8x on quantized
            // matmul against the python wheel's CI-built MLX.
            .define("CMAKE_OSX_DEPLOYMENT_TARGET", "15.0")
            .build()
    };

    // MLX's runtime Metal JIT selects -std=metal4.0 behind
    // __builtin_available(macOS 26, ...), which misfires on macOS 15 with the
    // Xcode 26 toolchain and aborts kernel compilation ("invalid value
    // 'metal4.0'"). Cap the JIT language version at 3.2, which every MLX
    // kernel supports. The MLX source is FetchContent'd during the cmake run,
    // so patch after the first build and rebuild if the bad branch is present.
    let patch_metal_version = |out_dir: &std::path::Path| -> bool {
        let device_cpp = out_dir.join("build/_deps/mlx-src/mlx/backend/metal/device.cpp");
        let Ok(source) = std::fs::read_to_string(&device_cpp) else {
            return false;
        };
        if !source.contains("MTL::LanguageVersion4_0") {
            return false;
        }
        let patched = source.replace(
            "return MTL::LanguageVersion4_0;",
            "return MTL::LanguageVersion3_2; // spqx: metal4.0 JIT breaks on macOS 15 + Xcode 26",
        );
        std::fs::write(&device_cpp, patched).expect("failed to patch mlx device.cpp");
        true
    };

    let mut dst = build_mlx_c();
    if patch_metal_version(&dst) {
        dst = build_mlx_c();
    }

    // Link paths
    let lib_dir = dst.join("lib");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());

    // Also check lib64 (some CMake configs use this)
    let lib64_dir = dst.join("lib64");
    if lib64_dir.exists() {
        println!("cargo:rustc-link-search=native={}", lib64_dir.display());
    }

    // Link mlx-c and mlx static libraries
    println!("cargo:rustc-link-lib=static=mlxc");
    println!("cargo:rustc-link-lib=static=mlx");

    // Link macOS system frameworks required by MLX
    println!("cargo:rustc-link-lib=framework=Metal");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=Accelerate");
    println!("cargo:rustc-link-lib=framework=MetalPerformanceShaders");

    // Link C++ standard library
    println!("cargo:rustc-link-lib=c++");

    // MLX's `__builtin_available(macOS 26, ...)` guard emits a call to
    // `___isPlatformVersionAtLeast`, which lives in clang's compiler-rt
    // builtins. Rust links with `-nodefaultlibs`, so we must add it
    // explicitly or the final binary fails with an undefined symbol.
    if let Some(dir) = clang_rt_dir() {
        println!("cargo:rustc-link-search=native={dir}");
        println!("cargo:rustc-link-lib=static=clang_rt.osx");
    }

    // Rerun if mlx-c sources change
    println!("cargo:rerun-if-changed=../../mlx-c/CMakeLists.txt");
    println!("cargo:rerun-if-changed=../../mlx-c/mlx/c/");
}

/// Directory holding `libclang_rt.osx.a` for the active clang toolchain.
#[cfg(feature = "mlx")]
fn clang_rt_dir() -> Option<String> {
    let output = std::process::Command::new("clang")
        .arg("-print-resource-dir")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let resource_dir = String::from_utf8(output.stdout).ok()?;
    let dir = std::path::Path::new(resource_dir.trim()).join("lib/darwin");
    dir.join("libclang_rt.osx.a")
        .exists()
        .then(|| dir.display().to_string())
}
