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
    logos-module-builder.lib.mkLogosModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
    };
}
