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
      url = "git+https://github.com/caniko/plinth.git?ref=refs/heads/trunk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    harbor-rs = {
      url = "git+https://github.com/caniko/harbor-rs.git?ref=trunk&rev=c4ffa5d9b9232eae1f6693dfbcbcdd2548f31592";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-overlay.follows = "rust-overlay";
      inputs.crane.follows = "crane";
    };
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
    harbor-rs,
    ...
  }:
  # Nixpkgs 26.11 retired x86_64-darwin; retain its supported native systems.
    flake-utils.lib.eachSystem ["x86_64-linux" "aarch64-linux" "aarch64-darwin"] (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [(import rust-overlay)];
      };
      lib = pkgs.lib;
      version = (builtins.fromTOML (builtins.readFile ./crates/detritus-server/Cargo.toml)).package.version;
      msrv = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.rust-version;
      msrvToolchain = pkgs.rust-bin.stable."${msrv}.0".default;
      msrvCraneLib = (crane.mkLib pkgs).overrideToolchain (_: msrvToolchain);
      toolchain = harbor-rs.lib.mkToolchain {
        inherit pkgs;
        channel = "nightly";
        date = "latest";
        extensions = ["rust-src" "rustfmt" "clippy" "llvm-tools-preview"];
        crossTargets = ["x86_64-unknown-linux-gnu" "aarch64-unknown-linux-gnu"];
      };
      rustToolchain = toolchain.rustToolchain;
      craneLib = toolchain.craneLib;
      cross = harbor-rs.lib.mkCross {
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
        inherit src nativeBuildInputs version;
        pname = "detritus";
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
            homepage = "https://github.com/caniko/rs-detritus";
            license = pkgs.lib.licenses.asl20;
            mainProgram = "detritusd";
          };
        });

      crossPackageSet = harbor-rs.lib.mkCrossPackages ({
          inherit pkgs craneLib cross commonArgs;
          pname = "detritus";
          targets = ["native" "aarch64-linux"];
        }
        // lib.optionalAttrs (builtins.hasAttr "toolchainArgs" (builtins.functionArgs harbor-rs.lib.mkCrossPackages)) {
          toolchainArgs = {
            channel = "stable";
            extensions = ["rust-src" "rustfmt" "clippy"];
          };
        });

      docs = pkgs.stdenv.mkDerivation {
        pname = "detritus-docs";
        inherit version;
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
      packages =
        {
          default = detritus;
          inherit detritus docs website;
          site = website;
        }
        // lib.optionalAttrs (system == "x86_64-linux") {
          "detritus-aarch64-linux" = crossPackageSet."detritus-aarch64-linux";
        };

      apps.deploy-pages = plinth.lib.${system}.mkDeployPagesApp {
        domain = "detritus.tartanoglu.com";
      };

      checks = {
        default = detritus;
        inherit detritus;
        formatting = treefmtEval.config.build.check self;
        fmt = craneLib.cargoFmt {
          inherit src version;
          pname = "detritus";
        };
        clippy = craneLib.cargoClippy (commonArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });
        # Fail if flake inputs ever point at the retired Codeberg/Codefloe
        # mirrors again (fleet migrated to github.com/caniko/*).
        # sourceUrl package metadata is excluded: informational only, not fetched.
        host-pinning = let
          # Split across literals so this file never matches its own pattern.
          staleHosts = "cod" + "eberg|cod" + "efloe";
        in
          pkgs.runCommand "rs-detritus-host-pinning" {} ''
            if ${pkgs.lib.getExe pkgs.ripgrep} -v "sourceUrl" ${./flake.nix} ${./flake.lock} \
              | ${pkgs.lib.getExe pkgs.ripgrep} -q "${staleHosts}"; then
              echo "ERROR: retired forge host in flake inputs:" >&2
              ${pkgs.lib.getExe pkgs.ripgrep} -v "sourceUrl" ${./flake.nix} ${./flake.lock} \
                | ${pkgs.lib.getExe pkgs.ripgrep} -n "${staleHosts}" >&2 || true
              exit 1
            fi
            touch $out
          '';
      };

      formatter = treefmtEval.config.build.wrapper;

      devShells =
        (harbor-rs.lib.mkDevShells {
          inherit pkgs craneLib cross;
          enableOsxcrossEnv = false;
          packages =
            [
              pkgs.cargo-llvm-cov
              pkgs.cargo-nextest
              pkgs.mdbook
              pkgs.pkg-config
              pkgs.pre-commit
              pkgs.protobuf
            ]
            ++ pre-commit-check.enabledPackages;
          extraShellHook = pre-commit-check.shellHook;
        })
        // {
          msrv = harbor-rs.lib.mkDevShell {
            inherit pkgs cross;
            craneLib = msrvCraneLib;
            cargoConfig = null;
            enableWindowsEnv = false;
            enableOsxcrossEnv = false;
            extraEnv = {
              RUSTC = "${msrvToolchain}/bin/rustc";
              RUSTDOC = "${msrvToolchain}/bin/rustdoc";
              RUSTFLAGS = "";
              CARGO_ENCODED_RUSTFLAGS = "";
              CARGO_TARGET_DIR = "target/msrv";
            };
          };
          docs = harbor-rs.lib.mkDocsShell {
            inherit pkgs craneLib cross;
            extraEnv.RUSTDOCFLAGS = "-D warnings";
          };
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
