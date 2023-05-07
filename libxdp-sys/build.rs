extern crate bindgen;
use which::which;

use anyhow::{anyhow, Result};
use std::{env, path::PathBuf, process::Command};

fn build_bpftool() -> Result<PathBuf> {
    let src_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let compile_result = Command::new("make")
        .current_dir(src_path.join("bpftool/src"))
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
        Ok(src_path.join("bpftool/src/bpftool"))
    }
}

fn main() -> anyhow::Result<()> {
    let src_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    const XDP_LIBRARY_SUFFIX: &str = "xdp-tools/lib/libxdp/libxdp.a";
    const BPF_LIBRARY_SUFFIX: &str = "xdp-tools/lib/libbpf/src/libbpf.a";

    which("m4").expect("m4 is missing");
    which("clang").expect("clang is missing");

    let bpftool = build_bpftool()?;

    let compile_result = Command::new("make")
        .arg("libxdp")
        .env("BPFTOOL", bpftool)
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

    if !src_path.join(XDP_LIBRARY_SUFFIX).exists() || !src_path.join(BPF_LIBRARY_SUFFIX).exists() {
        eprintln!("unable to find libxdp or libbpf libraries");
        return Err(anyhow::anyhow!("unable to find libxdp or libbpf libraries"));
    }

    println!(
        "cargo:rustc-link-search={}",
        src_path.join("xdp-tools/lib/libxdp").display()
    );
    println!("cargo:rustc-link-lib=static=xdp");

    println!(
        "cargo:rustc-link-search={}",
        src_path.join("xdp-tools/lib/libbpf/src").display()
    );
    println!("cargo:rustc-link-lib=static=bpf");

    println!("cargo:rustc-link-lib=elf");
    println!("cargo:rustc-link-lib=z");

    println!("cargo:rerun-if-changed=wrapper.h");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .generate_inline_functions(true)
        .clang_arg(format!(
            "-I{}",
            src_path
                .join("xdp-tools/lib/libbpf/src/root/usr/include")
                .display()
        ))
        .clang_arg(format!(
            "-I{}",
            src_path.join("xdp-tools/headers").display()
        ))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("unable to generate bindings");

    bindings
        .write_to_file(src_path.join("src/bindings.rs"))
        .expect("couldn't write bindings!");

    Ok(())
}
