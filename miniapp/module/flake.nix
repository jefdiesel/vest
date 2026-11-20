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
  };

  outputs = inputs@{ logos-module-builder, ... }:
    let
      # risc0 (the in-process prover of private claims) compiles its Apple GPU kernels with the
      # system's Metal compiler (`xcrun metal`, from Xcode), which a nix build doesn't see: this
      # xcrun hands the call to macOS's own. Apple-silicon macOS builds only.
      pkgs = logos-module-builder.inputs.nixpkgs.legacyPackages.aarch64-darwin;
      systemXcrun = pkgs.writeShellScriptBin "xcrun" ''
        unset DEVELOPER_DIR SDKROOT
        exec /usr/bin/xcrun "$@"
      '';
      # rapidsnark's build script reads a file outside its crate, which the vendored copy lacks:
      # its prebuilt static archives (the release its pinned version names) come from here.
      rapidsnark = pkgs.runCommand "rapidsnark-macOS-arm64-v0.0.8" { nativeBuildInputs = [ pkgs.unzip ]; } ''
        unzip -q ${pkgs.fetchurl {
          url = "https://github.com/iden3/rapidsnark/releases/download/v0.0.8/rapidsnark-macOS-arm64-v0.0.8.zip";
          sha256 = "dbd2c1498663223232f9c3ad02259d2839e62e784e9b1f6a0e9bd5070443990d";
        }}
        mkdir -p $out
        cp rapidsnark-macOS-arm64-v0.0.8/lib/*.a $out/
      '';
    in
    logos-module-builder.lib.mkLogosModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
      rustExtraNativeBuildInputs = [ systemXcrun ];
      # risc0's kernel build keeps a cache under $HOME, which a nix build has read-only.
      rustEnv = {
        HOME = "/private/tmp/vest-module-build-home";
        RAPIDSNARK_LIB_DIR = "${rapidsnark}";
      };
    };
}
