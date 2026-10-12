# The Lodestar agent on nix-darwin: a launchd daemon (label org.lodestar.agent) and the
# activation audit hook. Exported as `darwinModules.agent`.
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
  log = "/var/log/lodestar-agent.log";
in
{
  imports = [
    (import ./common.nix {
      inherit packagesFor;
      isDarwin = true;
    })
  ];

  config = lib.mkIf cfg.enable {
    launchd.daemons.lodestar-agent.serviceConfig = {
      Label = "org.lodestar.agent";
      ProgramArguments = [
        (lib.getExe cfg.package)
        "serve"
        "--config"
        "/etc/lodestar-agent/config.json"
      ];
      RunAtLoad = true;
      KeepAlive = true;
      # The detached apply must survive a restart of the daemon.
      AbandonProcessGroup = true;
      EnvironmentVariables = {
        PATH = daemonPath;
        HOME = "/var/root";
        # Changes with the config, so nix-darwin reloads the daemon (like restartTriggers).
        LODESTAR_CONFIG_HASH =
          builtins.hashString "sha256"
            config.environment.etc."lodestar-agent/config.json".text;
      };
      StandardOutPath = log;
      StandardErrorPath = log;
    };

    system.activationScripts.postActivation.text = lib.mkAfter ''
      mkdir -p /var/lib/lodestar-agent
      ${import ./audit-hook.nix { inherit pkgs; }}
    '';
  };
}
