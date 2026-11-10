# lodestar.lib.hostsFrom { nixosConfigurations ? {}, darwinConfigurations ? {}, tailnet ? null }
#
# The value for `services.lodestar-server.hosts`: every configuration that enables
# `services.lodestar-agent`, keyed by the agent's `hostName`, with the channel, OS, Nix
# system and user read from that configuration, and the Tailscale identity the server binds
# the host to: `nodeId` from the agent's `tailscale.nodeId`, `dnsName` from its
# `tailscale.dnsName`, else (only when it has no nodeId either) `<hostName>.<tailnet>` when
# `tailnet` (the tailnet's MagicDNS domain, e.g. "example.ts.net") is given. A host with
# neither fails the server's assertion.
{ lib }:
{
  nixosConfigurations ? { },
  darwinConfigurations ? { },
  tailnet ? null,
}:
let
  fromConfigs =
    os: configs:
    lib.mapAttrs' (
      _: c:
      let
        agent = c.config.services.lodestar-agent;
        identity = agent.tailscale or { };
        nodeId = identity.nodeId or null;
        # Derived only for a host with no explicit identity: a derived name next to a pinned
        # nodeId would bind the host to both, and fail it whenever its machine name differs.
        dnsName =
          if (identity.dnsName or null) != null then
            identity.dnsName
          else if nodeId == null && tailnet != null then
            "${agent.hostName}.${tailnet}"
          else
            null;
      in
      lib.nameValuePair agent.hostName {
        inherit (agent) channel user;
        inherit os dnsName nodeId;
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
