{
  description = "Vesting core module for Basecamp: privacy-preserving token vesting on LEZ 0.4 (RFP-017)";

  nixConfig = {
    extra-substituters = [ "https://cache.nix.logos.co/public" ];
    extra-trusted-public-keys = [ "public:l4HrXgL4nw246+LBh2SOJyhz64BoGegOYLheT/iIAPU=" ];
  };

  # Build from a STAGED copy (miniapp/module/stage.sh): the builder compiles only rust-lib/, so
  # the repository crates it uses (sdk, core, program, vendor, miniapp/module/core) are copied
  # into rust-lib/vest/ first.
  inputs = {
    # The builder revision Basecamp 0.3.x locks (as the Names and LEZ Bridge modules use).
    logos-module-builder.url = "github:logos-co/logos-module-builder/de169fd45d7bdf6e28ce1f151b4c34880523ace9";
    # A newer rust-overlay than the builder pins, for the rustc LEZ rc3 needs (1.98.1; the
    # builder's own overlay stops at 1.94.1). It only picks the crate's compiler; the builder,
    # the protocol and Qt stay at the revision Basecamp locks.
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "logos-module-builder/nixpkgs";
    logos-module-builder.inputs.rust-overlay.follows = "rust-overlay";
  };

  outputs = inputs@{ logos-module-builder, ... }:
    let
      inherit (logos-module-builder.inputs.nixpkgs) lib;
      # LEZ crates embed their guest ELFs from an `artifacts/` folder above the crate
      # (build_utils::include_artifacts), which a vendored crate has not. The setup hook puts the
      # rc3 checkout's own artifacts at the build's top folder, above every vendored crate.
      lezSrc = builtins.fetchGit {
        url = "https://github.com/logos-blockchain/logos-execution-zone.git";
        rev = "bafd07be0701398f9bc435f73ab05e9a7936c301"; # v0.4.0-rc3
        allRefs = true;
      };
      # rapidsnark's build script reads a file outside its crate, which the vendored copy lacks:
      # its prebuilt static archives (the release its pinned version names) come from here. macOS
      # takes iden3's own release; Linux takes the -fPIC rebuilds the Logos fork of the crate
      # publishes (iden3's Linux archives are not PIC, so they cannot go into a shared plugin).
      rapidsnarkZip = {
        aarch64-darwin = {
          name = "rapidsnark-macOS-arm64-v0.0.8";
          url = "https://github.com/iden3/rapidsnark/releases/download/v0.0.8/rapidsnark-macOS-arm64-v0.0.8.zip";
          sha256 = "dbd2c1498663223232f9c3ad02259d2839e62e784e9b1f6a0e9bd5070443990d";
        };
        x86_64-linux = {
          name = "rapidsnark-linux-x86_64-pic-v0.0.8";
          url = "https://github.com/logos-blockchain/logos-blockchain-rust-rapidsnark/releases/download/rapidsnark-pic-v0.0.8/rapidsnark-linux-x86_64-pic-v0.0.8.zip";
          sha256 = "59bdd709eed96235de061f352893f4650c923b54b591052118593012bb1cd831";
        };
        aarch64-linux = {
          name = "rapidsnark-linux-aarch64-pic-v0.0.8";
          url = "https://github.com/logos-blockchain/logos-blockchain-rust-rapidsnark/releases/download/rapidsnark-pic-v0.0.8/rapidsnark-linux-aarch64-pic-v0.0.8.zip";
          sha256 = "5f659af82991813c57e761f2ac6b23335591c86ee911c2f5841772c78af0999f";
        };
      };
      systems = builtins.attrNames rapidsnarkZip;
      # The Logos blockchain circuits (in the LEZ wallet's dependencies) download their prebuilt
      # libraries in a build script, which a sandboxed Linux build cannot: the same release
      # archive is fetched here and handed in through LBC_ROOT_DIR. (macOS builds unsandboxed
      # and keeps the build script's own download.)
      circuitsTarball = {
        x86_64-linux = {
          url = "https://github.com/logos-blockchain/logos-blockchain-circuits/releases/download/v0.5.7/logos-blockchain-circuits-v0.5.7-linux-x86_64.tar.gz";
          sha256 = "92a0c6ad810d486631518b7b9b247a2b66f66af6f8f8ddce381acbbdfedc0f7f";
        };
        aarch64-linux = {
          url = "https://github.com/logos-blockchain/logos-blockchain-circuits/releases/download/v0.5.7/logos-blockchain-circuits-v0.5.7-linux-aarch64.tar.gz";
          sha256 = "1eaa3e339d85d741dd70314c4ad45b7ee67c2baaf6c55e35170801161c372d7b";
        };
      };

      # The builder takes one set of crate inputs for every system, so the module is built once
      # per system with that system's own inputs, and each system's outputs are taken from its
      # own build.
      moduleFor = system:
        let
          pkgs = logos-module-builder.inputs.nixpkgs.legacyPackages.${system};
          z = rapidsnarkZip.${system};
          rapidsnark = pkgs.runCommand z.name { nativeBuildInputs = [ pkgs.unzip ]; } ''
            unzip -q ${pkgs.fetchurl { inherit (z) url sha256; }}
            mkdir -p $out
            cp ${z.name}/lib/*.a $out/
          '';
          lezArtifactsHook = pkgs.makeSetupHook { name = "lez-rc3-artifacts"; } (pkgs.writeText "lez-rc3-artifacts.sh" ''
            if [ -n "''${NIX_BUILD_TOP:-}" ] && [ ! -e "$NIX_BUILD_TOP/artifacts" ]; then
              cp -r ${lezSrc}/artifacts "$NIX_BUILD_TOP/artifacts"
              chmod -R u+w "$NIX_BUILD_TOP/artifacts"
            fi
          '');
          # risc0 (the in-process prover of private claims) compiles its Apple GPU kernels with the
          # system's Metal compiler (`xcrun metal`, from Xcode), which a nix build doesn't see: this
          # xcrun hands the call to macOS's own.
          systemXcrun = pkgs.writeShellScriptBin "xcrun" ''
            unset DEVELOPER_DIR SDKROOT
            exec /usr/bin/xcrun "$@"
          '';
          pcsc = lib.getLib pkgs.pcsclite;
          circuits = pkgs.runCommand "logos-blockchain-circuits-v0.5.7" { } ''
            mkdir -p $out
            tar -xzf ${pkgs.fetchurl circuitsTarball.${system}} -C $out --strip-components=1
          '';
          perOs =
            if pkgs.stdenv.isDarwin then {
              rustExtraNativeBuildInputs = [ systemXcrun lezArtifactsHook ];
              # risc0's kernel build keeps a cache under $HOME, which a nix build has read-only.
              rustEnv = {
                HOME = "/private/tmp/vest-module-build-home";
                RAPIDSNARK_LIB_DIR = "${rapidsnark}";
              };
            } else {
              # Linux: risc0 proves on the CPU (its C++ kernels; no Metal, and no CUDA feature).
              # librocksdb-sys (in the LEZ wallet's dependencies) runs bindgen, which needs libclang;
              # pcsc-sys (Keycard support) takes PC/SC from PCSC_LIB_DIR.
              rustExtraNativeBuildInputs = [ lezArtifactsHook pkgs.rustPlatform.bindgenHook ];
              rustEnv = {
                RAPIDSNARK_LIB_DIR = "${rapidsnark}";
                PCSC_LIB_DIR = "${pcsc}/lib";
                LBC_ROOT_DIR = "${circuits}";
              };
              # The plugin links PC/SC (CMakeLists.txt); the portable bundle then carries it.
              extraBuildInputs = [ pkgs.pcsclite ];
            };
        in
        logos-module-builder.lib.mkLogosModule ({
          src = ./.;
          configFile = ./metadata.json;
          flakeInputs = inputs;
        } // perOs);

      bySystem = lib.genAttrs systems moduleFor;
      darwin = bySystem.aarch64-darwin;
      pick = attr: lib.genAttrs systems (system: bySystem.${system}.${attr}.${system});
    in
    darwin // {
      packages = pick "packages";
      devShells = pick "devShells";
      configFor = pick "configFor";
    } // lib.optionalAttrs (darwin ? checks) { checks = pick "checks"; }
      // lib.optionalAttrs (darwin ? apps) { apps = pick "apps"; };
}
