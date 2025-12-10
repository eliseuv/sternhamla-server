{
  description = "Rust project flake";

  nixConfig = {
    extra-substituters = [
      "https://cache.nixos.org"
      "https://nix-community.cachix.org"
    ];
    extra-trusted-public-keys = [
      "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
      "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="
    ];
  };

  inputs = {
    nixpkgs.url = "nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      fenix,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let

        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };

        rustToolchain =
          with fenix.packages.${system};
          (combine (
            with stable;
            [
              rustc
              cargo
              rust-src
              rustfmt
              clippy
              rust-analyzer
            ]
          ));

      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
          ]
          ++ (with pkgs; [

            bacon

            # # Cargo tools
            # cargo-watch
            # cargo-cross
            # cargo-fuzz
            # cargo-nextest
            # cargo-deny
            # cargo-edit

            # Compilation cache
            sccache

            # # Debugging
            # lldb

            # # https://nixos.wiki/wiki/Rust#Building_Rust_crates_that_require_external_system_libraries
            openssl.dev
            pkg-config
          ]);

          # Explicitly tell rust-analyzer where to find the Rust source code
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          # Compilation cache
          RUSTC_WRAPPER = "sccache";
          # OpenSSL config
          # PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";

          shellHook = ''
            cargo --version
          '';
        };
      }
    );

}
