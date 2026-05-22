{
  description = "detritus crash and log receiver";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    git-hooks.url = "github:cachix/git-hooks.nix";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
    crane,
    treefmt-nix,
    git-hooks,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };
        lib = pkgs.lib;
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = ["rustfmt" "clippy"];
        };
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
        src = pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [
            (craneLib.fileset.commonCargoSources ./.)
            ./crates/detritus-client/tests
            ./crates/detritus-protocol/proto
            ./crates/detritus-protocol/tests
            ./crates/detritus-server/tests
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
          SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
          NIX_SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        treefmtEval = treefmt-nix.lib.evalModule pkgs (import ./nix/treefmt.nix);
        pre-commit-check = git-hooks.lib.${system}.run {
          src = ./.;
          hooks = import ./nix/pre-commit.nix {
            inherit pkgs rustToolchain;
            treefmtWrapper = treefmtEval.config.build.wrapper;
          };
        };

        detritus = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          meta = {
            description = "Lightweight crash and log receiver";
            homepage = "https://codeberg.org/caniko/rs-detritus";
            license = pkgs.lib.licenses.asl20;
            mainProgram = "detritusd";
          };
        });

        docs = pkgs.stdenv.mkDerivation {
          pname = "detritus-docs";
          version = "0.1.0";
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.maybeMissing ./docs;
          };
          nativeBuildInputs = [ pkgs.mdbook ];
          phases = [ "buildPhase" "installPhase" ];
          buildPhase = ''
            mkdir docs
            cp -r --no-preserve=mode "$src"/docs/. docs/
            mdbook build docs
          '';
          installPhase = ''
            cp -r docs/book "$out"
          '';
        };
      in
      {
        packages = {
          default = detritus;
          inherit detritus docs;
          site = docs;
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

        formatter = treefmtEval.config.build.wrapper;

        devShells.default = craneLib.devShell {
          checks = builtins.removeAttrs self.checks.${system} ["pre-commit"];
          packages = [
            pkgs.cargo-audit
            pkgs.cargo-deny
            pkgs.cargo-nextest
            pkgs.mdbook
            pkgs.pkg-config
            pkgs.pre-commit
            pkgs.protobuf
            pkgs.rust-analyzer
          ] ++ pre-commit-check.enabledPackages;
          shellHook = pre-commit-check.shellHook;
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

      nixosConfigurations.detritus-multi-test-vm = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          (import ./nix/test-vm-multi.nix { inherit self; })
        ];
      };
    };
}
