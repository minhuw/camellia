{
  description = "Camellia: a natural Rust library for XDP socket";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    nixos-generators = {
      url = "github:nix-community/nixos-generators";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
      nixos-generators,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
      in
      with pkgs;
      {
        colmena = {
          meta = {
            nixpkgs = import nixpkgs { inherit system overlays; };
          };

          testbed = {
            deployment = {
              targetHost = "127.0.0.1";
            };
            imports = [
              ./nix/configuration.nix
            ];
          };
        };

        devShells.default = mkShell {
          buildInputs = [
            colmena
            pkg-config
            bpftools
            bpftrace
            elfutils
            ethtool
            libcap
            libpcap
            valgrind
            llvmPackages_15.clangUseLLVM
            llvmPackages_15.libllvm
            llvmPackages_15.libclang
            rust-bin.stable.latest.default
            m4
            zlib
            iperf
            nixfmt-rfc-style
          ];

          CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER = "sudo -E";
          LIBCLANG_PATH = "${pkgs.llvmPackages_15.libclang.lib}/lib";

          hardeningDisable = [ "all" ];
        };

        packages = {
          dev-image = nixos-generators.nixosGenerate {
            system = "x86_64-linux";
            modules = [ ./nix/configuration.nix ];
            format = "qcow";

            specialArgs = {
              diskSize = 32 * 1024;
            };
          };
        };
      }
    );
}
