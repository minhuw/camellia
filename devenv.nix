{ pkgs, ... }:
let
  stdenv = pkgs.stdenv;
  lib = pkgs.lib;
in
{
  packages =
    with pkgs;
    [
      bear
      cmake
      elfutils
      git
      libcap
      libpcap
      (hiPrio gcc)
      linuxHeaders
      llvmPackages_15.clangUseLLVM
      llvmPackages_15.libllvm
      llvmPackages_15.libclang
      flamegraph
      m4
      python3
      rustup
      cargo
      strace
      tokei
      openssh
      pkg-config
      which
      zlib
    ]
    ++ [
      iperf3
      ethtool
      pkgs.linuxPackages_latest.perf
    ];

  # https://devenv.sh/pre-commit-hooks/
  pre-commit.hooks.shellcheck.enable = true;
  pre-commit.hooks.nixfmt.enable = true;
  pre-commit.hooks.cargo-check.enable = true;

  # From: https://github.com/NixOS/nixpkgs/blob/1fab95f5190d087e66a3502481e34e15d62090aa/pkgs/applications/networking/browsers/firefox/common.nix#L247-L253
  # Set C flags for Rust's bindgen program. Unlike ordinary C
  # compilation, bindgen does not invoke $CC directly. Instead it
  # uses LLVM's libclang. To make sure all necessary flags are
  # included we need to look in a few places.
  enterShell = ''
    export LD_LIBRARY_PATH=''${LD_LIBRARY_PATH%:}
    export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER='sudo -E'
    export LIBCLANG_PATH="${pkgs.llvmPackages_15.libclang.lib}/lib"
    export BINDGEN_EXTRA_CLANG_ARGS="$(< ${stdenv.cc}/nix-support/libc-crt1-cflags) \
      $(< ${stdenv.cc}/nix-support/libc-cflags) \
      $(< ${stdenv.cc}/nix-support/cc-cflags) \
      $(< ${stdenv.cc}/nix-support/libcxx-cxxflags) \
      ${lib.optionalString stdenv.cc.isClang "-idirafter ${stdenv.cc.cc}/lib/clang/${lib.getVersion stdenv.cc.cc}/include"} \
      ${lib.optionalString stdenv.cc.isGNU "-isystem ${stdenv.cc.cc}/include/c++/${lib.getVersion stdenv.cc.cc} -isystem ${stdenv.cc.cc}/include/c++/${lib.getVersion stdenv.cc.cc}/${stdenv.hostPlatform.config} -idirafter ${stdenv.cc.cc}/lib/gcc/${stdenv.hostPlatform.config}/${lib.getVersion stdenv.cc.cc}/include"} \
    "
  '';
}
