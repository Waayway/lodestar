# The submodule type of one service link, shared by `lodestar.links.<id>` (host optional,
# filled in by `linksFrom`) and `services.lodestar-server.links.<id>` (host required), so
# the two cannot drift. Arguments: `hostType`, the type of the `host` option, and optionally
# `hostDefault`, its default value (omitted: the option is required).
{ lib }:
args@{ hostType, ... }:
let
  inherit (lib) mkOption types;
  urlType = types.strMatching "https?://.+";
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
        type = urlType;
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
        // lib.optionalAttrs (args ? hostDefault) { default = args.hostDefault; }
      );
      check = {
        enable = mkOption {
          type = types.bool;
          default = true;
          description = "Probe this link.";
        };
        url = mkOption {
          type = types.nullOr urlType;
          default = null;
          description = "URL to probe instead of `url` (e.g. an internal address).";
        };
        expectStatus = mkOption {
          type = types.listOf (types.ints.between 100 599);
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
