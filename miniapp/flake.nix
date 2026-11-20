{
  description = "Vesting mini-app for Basecamp: the view of the vesting core module (LEZ vesting, RFP-017)";

  nixConfig = {
    extra-substituters = [ "https://cache.nix.logos.co/public" ];
    extra-trusted-public-keys = [ "public:l4HrXgL4nw246+LBh2SOJyhz64BoGegOYLheT/iIAPU=" ];
  };

  # Build from a STAGED copy (module/stage.sh), where this app and the core module are the
  # sibling folders app/ and module/; or pass --override-input vesting path:/abs/staged/module.
  inputs = {
    # The builder revision Basecamp 0.3.x locks.
    logos-module-builder.url = "github:logos-co/logos-module-builder/de169fd45d7bdf6e28ce1f151b4c34880523ace9";
    # The core module this view calls. The input name must match the dependency in metadata.json.
    vesting.url = "path:../module";
    vesting.inputs.logos-module-builder.follows = "logos-module-builder";
  };

  outputs = inputs@{ logos-module-builder, ... }:
    logos-module-builder.lib.mkLogosQmlModule {
      src = ./.;
      configFile = ./metadata.json;
      flakeInputs = inputs;
    };
}
