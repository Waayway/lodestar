# lodestar.lib.linksFrom { nixosConfigurations ? {}, darwinConfigurations ? {} }
#
# The value for `services.lodestar-server.links`: every configuration's `lodestar.links`,
# keyed by link id, with `host` defaulted to that configuration's agent `hostName`
# (the name `hostsFrom` uses). Configurations without the option are skipped.
{ lib }:
{
  nixosConfigurations ? { },
  darwinConfigurations ? { },
}:
let
  fromConfigs =
    configs:
    map (
      c:
      let
        hostName = c.config.services.lodestar-agent.hostName;
      in
      lib.mapAttrs (
        id: l:
        let
          host = if l.host != null then l.host else hostName;
        in
        assert lib.assertMsg (
          host != null && host != ""
        ) "lodestar.lib.linksFrom: link `${id}` has no host (set `host` or the configuration's hostName)";
        l // { inherit host; }
      ) c.config.lodestar.links
    ) (lib.filter (c: c.config ? lodestar) (lib.attrValues configs));

  perConfig = fromConfigs nixosConfigurations ++ fromConfigs darwinConfigurations;
  counts = lib.foldl' (
    acc: ids: lib.foldl' (a: id: a // { ${id} = (a.${id} or 0) + 1; }) acc (lib.attrNames ids)
  ) { } perConfig;
  duplicates = lib.attrNames (lib.filterAttrs (_: n: n > 1) counts);
in
assert lib.assertMsg (duplicates == [ ])
  "lodestar.lib.linksFrom: link ids declared by more than one configuration: ${toString duplicates}";
lib.foldl' (a: b: a // b) { } perConfig
