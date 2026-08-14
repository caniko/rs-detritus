{
  description = "detritus crash and log receiver";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    plinth = {
      url = "git+https://codeberg.org/caniko/plinth.git?ref=refs/heads/trunk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rs-harbor.url = "git+https://codeberg.org/caniko/rs-harbor.git?ref=trunk&rev=f209ddbca3fdbb0dc31fa3886ccc2ff7369c18ac";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
    crane,
    treefmt-nix,
    git-hooks,
    plinth,
    rs-harbor,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [(import rust-overlay)];
      };
      lib = pkgs.lib;
      rustToolchain = pkgs.rust-bin.stable.latest.default.override {
        extensions = ["rustfmt" "clippy"];
      };
      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
      cross = rs-harbor.lib.mkCross {
        inherit pkgs system;
        enableOsxcross = false;
      };
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

      detritus = craneLib.buildPackage (commonArgs
        // {
          inherit cargoArtifacts;
          meta = {
            description = "Lightweight crash and log receiver";
            homepage = "https://codeberg.org/caniko/rs-detritus";
            license = pkgs.lib.licenses.asl20;
            mainProgram = "detritusd";
          };
        });

      crossPackageSet = rs-harbor.lib.mkCrossPackages ({
        inherit pkgs craneLib cross commonArgs;
        pname = "detritus";
        targets = ["native" "aarch64-linux"];
      } // lib.optionalAttrs (builtins.hasAttr "toolchainArgs" (builtins.functionArgs rs-harbor.lib.mkCrossPackages)) {
        toolchainArgs = {
          channel = "stable";
          extensions = ["rust-src" "rustfmt" "clippy"];
        };
      });

      docs = pkgs.stdenv.mkDerivation {
        pname = "detritus-docs";
        version = "0.1.0";
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.maybeMissing ./docs;
        };
        nativeBuildInputs = [pkgs.mdbook];
        phases = ["buildPhase" "installPhase"];
        buildPhase = ''
          mkdir docs
          cp -r --no-preserve=mode "$src"/docs/. docs/
          mdbook build docs
        '';
        installPhase = ''
          cp -r docs/book "$out"
        '';
      };
      website = plinth.lib.${system}.mkProjectSite {
        pname = "detritus-website";
        domain = "detritus.tartanoglu.com";
        configPath = ./website/plinth-project.toml;
        docsPackage = docs;
      };
    in {
      packages = {
        default = detritus;
        inherit detritus docs website;
        "detritus-aarch64-linux" = crossPackageSet."detritus-aarch64-linux";
        site = website;
      };

      apps.deploy-pages = plinth.lib.${system}.mkDeployPagesApp {
        domain = "detritus.tartanoglu.com";
      };

      checks = {
        default = detritus;
        inherit detritus;
        formatting = treefmtEval.config.build.check self;
        fmt = craneLib.cargoFmt {
          inherit src;
          pname = "detritus";
          version = "0.1.0";
        };
        clippy = craneLib.cargoClippy (commonArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });
      };

      formatter = treefmtEval.config.build.wrapper;

      devShells.default = craneLib.devShell {
        checks = builtins.removeAttrs self.checks.${system} ["pre-commit"];
        packages =
          [
            pkgs.cargo-audit
            pkgs.cargo-deny
            pkgs.cargo-nextest
            pkgs.mdbook
            pkgs.pkg-config
            pkgs.pre-commit
            pkgs.protobuf
            pkgs.rust-analyzer
          ]
          ++ pre-commit-check.enabledPackages;
        shellHook = pre-commit-check.shellHook;
      };
    })
    // {
      crossPackages."x86_64-linux"."aarch64-linux".detritus = self.packages."x86_64-linux"."detritus-aarch64-linux";
      overlays.default = final: _prev: {
        detritus = self.packages.${final.stdenv.hostPlatform.system}.detritus;
      };

      nixosModules.default = import ./nix/module.nix {inherit self;};

      nixosConfigurations.detritus-test-vm = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          (import ./nix/test-vm.nix {inherit self;})
        ];
      };

      nixosConfigurations.detritus-multi-test-vm = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          (import ./nix/test-vm-multi.nix {inherit self;})
        ];
      };
    };
}
