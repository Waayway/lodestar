# The Lodestar agent on NixOS: a systemd service running the copy at binaryPath, its
# port opened on tailscale0, and the activation audit hook. Exported as `nixosModules.agent`.
{ packagesFor }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.lodestar-agent;
  daemonPath = "/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:/usr/bin:/bin:/usr/sbin:/sbin";
in
{
  imports = [
    (import ./common.nix {
      inherit packagesFor;
      isDarwin = false;
    })
  ];

  config = lib.mkIf cfg.enable {
    # The server reaches the agent over the tailnet only.
    networking.firewall.interfaces.tailscale0.allowedTCPPorts = [ cfg.port ];

    systemd.services.lodestar-agent = {
      description = "Lodestar agent";
      wantedBy = [ "multi-user.target" ];
      after = [
        "tailscaled.service"
        "network-online.target"
      ];
      wants = [ "network-online.target" ];
      environment.PATH = lib.mkForce daemonPath;
      # ExecStart is the fixed binaryPath, so a new package must restart the unit itself.
      restartTriggers = [
        config.environment.etc."lodestar-agent/config.json".source
        cfg.package
      ];
      # The apply runs in its own transient unit (lodestar-apply-*), so a restart of this
      # one (e.g. by the activation it triggers) does not kill it.
      serviceConfig = {
        ExecStart = "${cfg.binaryPath} serve --config /etc/lodestar-agent/config.json";
        StateDirectory = "lodestar-agent";
        Restart = "always";
        RestartSec = 5;
      };
    };

    # Copies the package to binaryPath; activation runs before units are (re)started.
    system.activationScripts.lodestarAgentBinary = {
      deps = [ ];
      text = import ./install-binary.nix {
        inherit pkgs lib;
        source = lib.getExe cfg.package;
        inherit (cfg) binaryPath;
      };
    };

    system.activationScripts.lodestarAudit = {
      deps = [ ];
      text = import ./audit-hook.nix { inherit pkgs; };
    };
  };
}
