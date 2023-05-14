extern crate bindgen;
use which::which;

use anyhow::{anyhow, Result};
use std::{env, path::{Path, PathBuf}, process::Command};

fn build_bpftool(out_path: &Path) -> Result<PathBuf> {
    let src_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_path: PathBuf = out_path.join("bpftool/");
    std::fs::create_dir_all(&out_path).unwrap();

    let compile_result = Command::new("make")
        .current_dir(src_path.join("bpftool/src"))
        .env("OUTPUT", out_path.clone())
        .output()
        .expect("unable to compile bpftool");

    if !compile_result.status.success() {
        eprintln!(
            "unable to compile bpftool\n stdout: {}, stderr: {}",
            String::from_utf8_lossy(&compile_result.stdout),
            String::from_utf8_lossy(&compile_result.stderr)
        );
        eprintln!("unable to compile bpftool");
        Err(anyhow!("unable to compile bpftool"))
    } else {
        Ok(out_path.join("bpftool"))
    }
}

fn main() -> anyhow::Result<()> {
    let src_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let lib_path = out_path.join("lib");
    let include_path = out_path.join("include");

    const XDP_LIBRARY_SUFFIX: &str = "lib/libxdp.a";
    const BPF_LIBRARY_SUFFIX: &str = "lib/libbpf.a";

    which("m4").expect("m4 is missing");
    which("clang").expect("clang is missing");

    let bpftool = build_bpftool(&out_path)?;

    let compiler = match cc::Build::new().try_get_compiler() {
        Ok(compiler) => compiler,
        Err(_) => panic!(
            "a C compiler is required to compile libxdp-sys"
        ),
    };

    let compile_result = Command::new("make")
        .arg("libxdp")
        .env("BPFTOOL", bpftool)
        .env("CC", compiler.path())
        .env("CFLAGS", compiler.cflags_env())
        .current_dir(src_path.join("xdp-tools"))
        .output()?;

    if !compile_result.status.success() {
        eprintln!(
            "unable to compile libxdp stdout: {}, stderr: {}",
            String::from_utf8_lossy(&compile_result.stdout),
            String::from_utf8_lossy(&compile_result.stderr)
        );
        eprintln!("unable to compile libxdp library");
        return Err(anyhow::anyhow!("unable to compile libxdp library"));
    }

    let install_result = Command::new("make")
        .arg("install")
        .env("PREFIX", "")
        .env("DESTDIR", out_path.clone())
        .current_dir(src_path.join("xdp-tools/lib/libxdp"))
        .output()?;

    if !install_result.status.success() {
        eprintln!(
            "unable to compile libxdp stdout: {}, stderr: {}",
            String::from_utf8_lossy(&compile_result.stdout),
            String::from_utf8_lossy(&compile_result.stderr)
        );
        eprintln!("unable to compile libxdp library");
        return Err(anyhow::anyhow!("unable to compile libxdp library"));
    }

    let install_result = Command::new("make")
        .arg("install")
        .env("DESTDIR", out_path.clone())
        .env("PREFIX", "")
        .env("LIBDIR", "/lib")
        .current_dir(src_path.join("xdp-tools/lib/libbpf/src"))
        .output()?;

    if !install_result.status.success() {
        eprintln!(
            "unable to compile libbpf stdout: {}, stderr: {}",
            String::from_utf8_lossy(&compile_result.stdout),
            String::from_utf8_lossy(&compile_result.stderr)
        );
        eprintln!("unable to compile or install libbpf library");
        return Err(anyhow::anyhow!("unable to compile libbpf library"));
    }

    if !out_path.join(XDP_LIBRARY_SUFFIX).exists() || !out_path.join(BPF_LIBRARY_SUFFIX).exists() {
        eprintln!("unable to find libxdp or libbpf libraries");
        return Err(anyhow::anyhow!("unable to find libxdp or libbpf libraries"));
    }

    println!("cargo:rustc-link-search=native={}", lib_path.display());
    println!("cargo:rustc-link-lib=static=xdp");
    println!("cargo:rustc-link-lib=static=bpf");

    println!("cargo:rustc-link-lib=elf");
    println!("cargo:rustc-link-lib=z");
    println!("cargo:rerun-if-changed=wrapper.h");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .generate_inline_functions(true)
        .clang_arg(format!("-I{}", include_path.display()))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("unable to generate bindings");

    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("couldn't write bindings!");

    Ok(())
}
