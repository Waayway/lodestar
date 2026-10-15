# `lodestar.links.<id>`: service links shown in the dashboard's Services tab. A plain data
# option (configures nothing), imported on every NixOS and nix-darwin configuration and
# collected with `lodestar.lib.linksFrom`. Exported as `nixosModules.links` / `darwinModules.links`.
{ lib }:
{
  options.lodestar.links = lib.mkOption {
    type = lib.types.attrsOf (
      import ../lib/linkType.nix { inherit lib; } {
        hostType = lib.types.nullOr lib.types.str;
        hostDefault = null;
      }
    );
    default = { };
    example = lib.literalExpression ''
      { jellyfin = { url = "https://jellyfin.example.com"; icon = "jellyfin"; }; }
    '';
    description = ''
      Web applications this configuration hosts. `host` defaults to the declaring
      configuration (filled in by `lodestar.lib.linksFrom`).
    '';
  };
}
