# The Lodestar agent on nix-darwin: a launchd daemon (label org.lodestar.agent) running
# the copy at binaryPath, signed with a local certificate for a stable code identity
# (macos.stableSigning), allowed in the application firewall, and the activation audit
# hook. Exported as `darwinModules.agent`.
{ packagesFor }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.lodestar-agent;
  inherit (lib) mkOption types;
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

  options.services.lodestar-agent.macos = {
    stableSigning = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Sign the agent at `binaryPath` with a self-signed certificate created once on
          this Mac, so macOS privacy permissions (TCC: Automation, App Data, ...) granted
          to the agent survive updates. Off: the copy keeps its ad-hoc signature and
          every new build is a new identity that macOS asks about again.
        '';
      };

      directory = mkOption {
        type = types.str;
        default = "/var/lib/lodestar-agent/signing";
        description = ''
          Root-only (0700) directory with the signing key and certificate (`key.pem`,
          `cert.pem`, `cert.sha1`), created on first activation. Remove it and switch to
          get a new certificate (macOS asks for the permissions once more).
        '';
      };
    };

    firewall.enable = mkOption {
      type = types.bool;
      default = true;
      description = ''
        When the macOS application firewall is on, allow incoming connections for
        `binaryPath` (`socketfilterfw --add` and `--unblockapp`) on every activation.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    launchd.daemons.lodestar-agent.serviceConfig = {
      Label = "org.lodestar.agent";
      ProgramArguments = [
        cfg.binaryPath
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
        # Change with the config and the package, so nix-darwin reloads the daemon
        # (like restartTriggers); the program path itself stays the same.
        LODESTAR_CONFIG_HASH =
          builtins.hashString "sha256"
            config.environment.etc."lodestar-agent/config.json".text;
        LODESTAR_PACKAGE = "${cfg.package}";
      };
      StandardOutPath = log;
      StandardErrorPath = log;
    };

    # extraActivation runs before nix-darwin (re)loads launchd daemons (`launchd`) and
    # before home-manager (`postActivation`), so the daemon starts from the new, signed
    # binary.
    system.activationScripts.extraActivation.text = lib.mkAfter (
      import ./install-binary.nix {
        inherit pkgs lib;
        source = lib.getExe cfg.package;
        inherit (cfg) binaryPath;
        darwin = {
          sign = cfg.macos.stableSigning.enable;
          dir = cfg.macos.stableSigning.directory;
          firewall = cfg.macos.firewall.enable;
        };
      }
    );

    system.activationScripts.postActivation.text = lib.mkAfter ''
      mkdir -p /var/lib/lodestar-agent
      ${import ./audit-hook.nix { inherit pkgs; }}
    '';
  };
}
