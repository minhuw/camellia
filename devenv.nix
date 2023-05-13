{ pkgs ? import (fetchTarball
  "https://github.com/NixOS/nixpkgs/archive/1a411f23ba299db155a5b45d5e145b85a7aafc42.tar.gz")
  { }, ... }:
let
  stdenv = pkgs.stdenv;
  lib = pkgs.lib;
in {
  # https://devenv.sh/packages/
  #   packages = with pkgs; [ 
  #     git 
  #     nixpkgs-fmt
  #     (hiPrio gcc)
  #     llvmPackages.clangUseLLVM
  #     llvmPackages.libllvm
  #     llvmPackages.libclang
  #     m4 
  #     elfutils 
  #     zlib 
  #     libpcap 
  #     bpftool 
  #     linuxHeaders 
  #     rustup 
  #   ];

  packages = with pkgs; [
    bear
    bpftool
    cmake
    elfutils
    git
    libpcap
    (hiPrio gcc)
    linuxHeaders
    llvmPackages_15.clangUseLLVM
    llvmPackages_15.libllvm
    llvmPackages_15.libclang
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

  # https://devenv.sh/pre-commit-hooks/
  pre-commit.hooks.shellcheck.enable = true;
  pre-commit.hooks.nixfmt.enable = true;

  # From: https://github.com/NixOS/nixpkgs/blob/1fab95f5190d087e66a3502481e34e15d62090aa/pkgs/applications/  networking/browsers/firefox/common.nix#L247-L253
  # Set C flags for Rust's bindgen program. Unlike ordinary C
  # compilation, bindgen does not invoke $CC directly. Instead it
  # uses LLVM's libclang. To make sure all necessary flags are
  # included we need to look in a few places.
  enterShell = ''
    export LD_LIBRARY_PATH=''${LD_LIBRARY_PATH%:}
    export LIBCLANG_PATH="${pkgs.llvmPackages_15.libclang.lib}/lib"
    export BINDGEN_EXTRA_CLANG_ARGS="$(< ${stdenv.cc}/nix-support/libc-crt1-cflags) \
      $(< ${stdenv.cc}/nix-support/libc-cflags) \
      $(< ${stdenv.cc}/nix-support/cc-cflags) \
      $(< ${stdenv.cc}/nix-support/libcxx-cxxflags) \
      ${
        lib.optionalString stdenv.cc.isClang
        "-idirafter ${stdenv.cc.cc}/lib/clang/${
          lib.getVersion stdenv.cc.cc
        }/include"
      } \
      ${
        lib.optionalString stdenv.cc.isGNU
        "-isystem ${stdenv.cc.cc}/include/c++/${
          lib.getVersion stdenv.cc.cc
        } -isystem ${stdenv.cc.cc}/include/c++/${
          lib.getVersion stdenv.cc.cc
        }/${stdenv.hostPlatform.config} -idirafter ${stdenv.cc.cc}/lib/gcc/${stdenv.hostPlatform.config}/${
          lib.getVersion stdenv.cc.cc
        }/include"
      } \
    "
  '';
}
