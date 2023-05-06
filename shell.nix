{ pkgs ? import (fetchTarball "https://github.com/NixOS/nixpkgs/archive/1a411f23ba299db155a5b45d5e145b85a7aafc42.tar.gz") { } }:
let
  llvmPackages = pkgs.llvmPackages_15;
in
pkgs.mkShell {
  hardeningDisable = [ "stackprotector" ];

  packages = with pkgs; [
    bear
    bpftool
    cmake
    elfutils
    git
    (hiPrio gcc)
    llvmPackages.clangUseLLVM
    llvmPackages.libllvm
    llvmPackages.libclang
    libpcap
    m4
    python3
    pkgconfig
    jq
    rustup
    strace
    openssh
    which
    zlib
  ];

  LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${llvmPackages.libclang.lib}/lib/clang/${pkgs.lib.getVersion llvmPackages.clangUseLLVM}/include";
}