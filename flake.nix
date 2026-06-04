{
  description = "Navix - keyboard-first Rust TUI for file navigation, preview overlays, and embedded shell workflows";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Single source of truth: read name/version straight from Cargo.toml so
        # the flake never drifts from the crate metadata.
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);

        navix = pkgs.rustPlatform.buildRustPackage {
          pname = cargoToml.package.name;
          version = cargoToml.package.version;

          src = self;

          # Pin dependencies to the checked-in lockfile for reproducible builds.
          cargoLock.lockFile = ./Cargo.lock;

          meta = {
            description = cargoToml.package.description;
            homepage = cargoToml.package.homepage;
            license = pkgs.lib.licenses.gpl3Plus;
            mainProgram = "navix";
          };
        };
      in
      {
        packages = {
          default = navix;
          navix = navix;
        };

        apps.default = {
          type = "app";
          program = "${navix}/bin/navix";
          meta.description = cargoToml.package.description;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ navix ];
          packages = [
            pkgs.rustfmt
            pkgs.clippy
            pkgs.rust-analyzer
          ];
        };

        formatter = pkgs.nixpkgs-fmt;
      }
    );
}
