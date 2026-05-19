{
  description = "detritus crash and log receiver";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    crane,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        craneLib = crane.mkLib pkgs;
        src = pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [
            (craneLib.fileset.commonCargoSources ./.)
            ./crates/detritus-protocol/proto
          ];
        };

        nativeBuildInputs = [
          pkgs.pkg-config
        ];

        commonArgs = {
          inherit src nativeBuildInputs;
          pname = "detritus";
          version = "0.1.0";
          cargoExtraArgs = "-p detritus-server";
          strictDeps = true;
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        detritus = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          meta = {
            description = "Lightweight crash and log receiver";
            homepage = "https://codeberg.org/caniko/rs-detritus";
            license = pkgs.lib.licenses.asl20;
            mainProgram = "detritusd";
          };
        });
      in
      {
        packages = {
          default = detritus;
          inherit detritus;
        };

        checks = {
          default = detritus;
          inherit detritus;
          fmt = craneLib.cargoFmt {
            inherit src;
            pname = "detritus";
            version = "0.1.0";
          };
          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};
          packages = [
            pkgs.cargo-nextest
            pkgs.pkg-config
            pkgs.protobuf
            pkgs.rust-analyzer
          ];
        };
      })
    // {
      overlays.default = final: _prev: {
        detritus = self.packages.${final.stdenv.hostPlatform.system}.detritus;
      };

      nixosModules.default = import ./nix/module.nix { inherit self; };

      nixosConfigurations.detritus-test-vm = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          (import ./nix/test-vm.nix { inherit self; })
        ];
      };
    };
}
