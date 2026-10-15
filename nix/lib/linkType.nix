# The submodule type of one service link, shared by `lodestar.links.<id>` (host optional,
# filled in by `linksFrom`) and `services.lodestar-server.links.<id>` (host required), so
# the two cannot drift. `hostType` is the type of the `host` option, `hostExtra` extra attributes for it (a default).
{ lib }:
{
  hostType,
  hostExtra ? { },
}:
let
  inherit (lib) mkOption types;
in
types.submodule (
  { name, ... }:
  {
    options = {
      title = mkOption {
        type = types.str;
        default = name;
        defaultText = lib.literalExpression "the attribute name";
        description = "Card title.";
      };
      url = mkOption {
        type = types.str;
        example = "https://jellyfin.example.com";
        description = "What the card links to.";
      };
      group = mkOption {
        type = types.str;
        default = "Services";
        description = "Section the card is shown in.";
      };
      description = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "One line under the title.";
      };
      icon = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "jellyfin";
        description = ''
          A Simple Icons slug or a Lodestar/Lucide icon name; null shows the title's first letter.
        '';
      };
      host = mkOption (
        {
          type = hostType;
          description = "Machine the service runs on.";
        }
        // hostExtra
      );
      check = {
        enable = mkOption {
          type = types.bool;
          default = true;
          description = "Probe this link.";
        };
        url = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = "URL to probe instead of `url` (e.g. an internal address).";
        };
        expectStatus = mkOption {
          type = types.listOf types.int;
          default = [
            200
            204
            301
            302
            303
            307
            308
            401
            403
          ];
          description = "HTTP statuses that count as up (a login page is up).";
        };
        insecureTls = mkOption {
          type = types.bool;
          default = false;
          description = "Accept self-signed certificates.";
        };
      };
    };
  }
)
