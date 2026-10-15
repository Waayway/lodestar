# Evaluation check of `lodestar.lib.linksFrom` over stub configurations built from
# nix/modules/links.nix: host defaulting, an explicit host kept, option defaults,
# duplicate ids failing, and configurations without the option skipped.
{
  pkgs,
  lib,
  linksFrom,
}:
let
  stub =
    name: links:
    lib.evalModules {
      modules = [
        (import ../modules/links.nix { inherit lib; })
        { options.services.lodestar-agent.hostName = lib.mkOption { default = name; }; }
        { lodestar.links = links; }
      ];
    };
  noOption = lib.evalModules {
    modules = [ { options.services.lodestar-agent.hostName = lib.mkOption { default = "plain"; }; } ];
  };

  r = linksFrom {
    nixosConfigurations = {
      alpha = stub "alpha" { a.url = "http://a"; };
      beta = stub "beta" {
        b = {
          url = "http://b";
          host = "pve";
        };
      };
      plain = noOption;
    };
  };
  dup = linksFrom {
    nixosConfigurations = {
      alpha = stub "alpha" { dup.url = "http://x"; };
      beta = stub "beta" { dup.url = "http://y"; };
    };
  };
in
assert r.a.host == "alpha";
assert r.a.title == "a";
assert r.a.group == "Services";
assert
  r.a.check.expectStatus == [
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
assert r.b.host == "pve";
assert
  lib.attrNames r == [
    "a"
    "b"
  ];
assert !(builtins.tryEval (builtins.deepSeq dup dup)).success;
pkgs.runCommand "links-from" { } "touch $out"
