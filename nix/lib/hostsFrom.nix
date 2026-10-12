# lodestar.lib.hostsFrom { nixosConfigurations ? {}, darwinConfigurations ? {} }
#
# The value for `services.lodestar-server.hosts`: every configuration that enables
# `services.lodestar-agent`, keyed by the agent's `hostName`, with the channel, OS, Nix
# system and user read from that configuration.
{ lib }:
{
  nixosConfigurations ? { },
  darwinConfigurations ? { },
}:
let
  fromConfigs =
    os: configs:
    lib.mapAttrs' (
      _: c:
      let
        agent = c.config.services.lodestar-agent;
      in
      lib.nameValuePair agent.hostName {
        inherit (agent) channel user;
        inherit os;
        system = c.pkgs.stdenv.hostPlatform.system;
      }
    ) (lib.filterAttrs (_: c: c.config.services.lodestar-agent.enable or false) configs);

  nixos = fromConfigs "linux" nixosConfigurations;
  darwin = fromConfigs "darwin" darwinConfigurations;
  duplicates = builtins.attrNames (builtins.intersectAttrs nixos darwin);
in
assert lib.assertMsg (duplicates == [ ])
  "lodestar.lib.hostsFrom: host names used by both a NixOS and a darwin configuration: ${toString duplicates}";
nixos // darwin
