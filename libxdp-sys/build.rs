extern crate bindgen;

use std::{env, path::PathBuf, process::Command};

fn main() -> anyhow::Result<()> {
    let src_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

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

    let compile_result = Command::new("make")
        .arg("libxdp")
        .current_dir(src_path.join("xdp-tools"))
        .status()?;
    if !compile_result.success() {
        eprintln!("unable to compile libxdp library");
        return Ok(());
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

    bindings
        .write_to_file(src_path.join("src/bindings.rs"))
        .expect("couldn't write bindings!");

    Ok(())
}
