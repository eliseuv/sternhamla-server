{
  description = "Rust";

  nixConfig = {

    extra-substituters = [
      "https://cache.nixos.org"
      "https://nix-community.cachix.org"
    ];

    extra-trusted-public-keys = [
      "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="
    ];

  };

  inputs = {
    nixpkgs.url = "nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      fenix,
      ...
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        config.allowUnfree = true;
        overlays = [ self.overlays.default ];
      };
    in
    {

      overlays.default = final: prev: {
        rustToolchain =
          with fenix.packages.${prev.stdenv.hostPlatform.system};
          combine (
            with stable;
            [
              rustc
              cargo
              rust-src
              rustfmt
              clippy
            ]
          );
      };

      devShells.${system}.default = pkgs.mkShell {

        packages = with pkgs; [

          rustToolchain

          rust-analyzer
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
          # openssl.dev
          # pkg-config

        ];

        env = {
          # Compilation cache
          RUSTC_WRAPPER = "sccache";
          # Required by rust-analyzer
          RUST_SRC_PATH = "${pkgs.rustToolchain}/lib/rustlib/src/rust/library";
          # OpenSSL config
          # PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
        };

        shellHook = ''
          cargo --version
        '';

      };

    };

}
