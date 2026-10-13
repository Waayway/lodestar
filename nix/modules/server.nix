# Runs lodestar-server (the dashboard and control plane) as its own user and writes its
# JSON config to /etc/lodestar-server/config.json and the host list to
# /etc/lodestar-server/hosts.json. NixOS only; exported as `nixosModules.server`.
{ packagesFor }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.lodestar-server;
  inherit (lib) mkOption types;
  configPath = "/etc/lodestar-server/config.json";
  stateDir = "/var/lib/lodestar-server";

  hostType = types.submodule {
    options = {
      channel = mkOption {
        type = types.str;
        description = "Branch the host follows.";
      };
      os = mkOption {
        type = types.enum [
          "linux"
          "darwin"
        ];
        description = "Operating system of the host.";
      };
      system = mkOption {
        type = types.str;
        example = "x86_64-linux";
        description = "Nix system of the host.";
      };
      user = mkOption {
        type = types.str;
        description = "Account that owns the host's flake checkout.";
      };
    };
  };
in
{
  options.services.lodestar-server = {
    enable = lib.mkEnableOption "the Lodestar server (dashboard and control plane)";

    package = mkOption {
      type = types.package;
      default = (packagesFor pkgs.stdenv.hostPlatform.system).lodestar-server;
      defaultText = lib.literalExpression "lodestar.packages.\${pkgs.stdenv.hostPlatform.system}.lodestar-server";
      description = "The lodestar-server package (with the dashboard embedded).";
    };

    hosts = mkOption {
      type = types.attrsOf hostType;
      default = { };
      example = lib.literalExpression ''
        lodestar.lib.hostsFrom { inherit (self) nixosConfigurations darwinConfigurations; }
      '';
      description = ''
        The managed hosts, by host name (the agent's `hostName`). Usually computed with
        `lodestar.lib.hostsFrom` from every configuration that enables the agent.
      '';
    };

    repoUrl = mkOption {
      type = types.str;
      example = "https://github.com/alice/infra";
      description = "Git URL the server mirrors (read-only) for channel heads and commit subjects.";
    };

    pushUrl = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "git@github.com:alice/infra.git";
      description = "Git remote the server pushes promotions to; required unless `promote` is null.";
    };

    deployKeyFile = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = ''
        SSH key with write access to `pushUrl` (needed for promote); a path outside the
        Nix store, readable by the `lodestar-server` user, from a secrets module such
        as opnix (`services.onepassword-secrets.secrets.<name> = { reference = "op://…";
        owner = "lodestar-server"; mode = "0400"; };` and
        `deployKeyFile = config.services.onepassword-secrets.secretPaths.<name>;`) or
        sops-nix (`config.sops.secrets.<name>.path`).
      '';
    };

    listenAddress = mkOption {
      type = types.str;
      example = "100.64.0.1";
      description = ''
        This host's Tailscale IP address. With Tailscale access control the server refuses
        to listen on anything else.
      '';
    };

    listenPort = mkOption {
      type = types.port;
      default = 8080;
      description = "TCP port of the dashboard and API.";
    };

    allowedUsers = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = [ "alice@github" ];
      description = "Tailscale logins allowed to use the dashboard and API.";
    };

    allowedTags = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = [ "tag:admin" ];
      description = "Tailscale node tags allowed to use the dashboard and API.";
    };

    agentPort = mkOption {
      type = types.port;
      default = config.services.lodestar-agent.port or 7461;
      defaultText = lib.literalExpression "config.services.lodestar-agent.port or 7461";
      description = ''
        TCP port of the agents. One port for every host: set the same
        `services.lodestar-agent.port` everywhere.
      '';
    };

    promote = mkOption {
      type = types.nullOr (
        types.submodule {
          options = {
            from = mkOption {
              type = types.str;
              description = "Channel whose head is promoted.";
            };
            to = mkOption {
              type = types.str;
              description = "Channel that is fast-forwarded.";
            };
          };
        }
      );
      default = {
        from = "unstable";
        to = "stable";
      };
      description = "Promote fast-forwards `to` to a commit of `from`; null disables promote.";
    };

    testing = {
      staticAddresses = mkOption {
        type = types.nullOr (types.attrsOf types.str);
        default = null;
        description = "TEST ONLY (VM test): fixed host -> ip[:port] map instead of Tailscale reachability.";
      };
      disableAccess = mkOption {
        type = types.bool;
        default = false;
        description = "TEST ONLY (VM test): disable dashboard access control.";
      };
      pollSecs = mkOption {
        type = types.nullOr types.ints.positive;
        default = null;
        description = "TEST ONLY (VM test): agent poll interval in seconds; null keeps the server default (60).";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.promote == null || cfg.pushUrl != null;
        message = "services.lodestar-server.pushUrl must be set while promote is enabled (or set promote = null).";
      }
    ];

    users.users.lodestar-server = {
      isSystemUser = true;
      group = "lodestar-server";
      # ssh (StrictHostKeyChecking=accept-new) keeps its known_hosts under $HOME.
      home = stateDir;
    };
    users.groups.lodestar-server = { };

    # Lets the unprivileged server use `tailscale status` / `tailscale whois`.
    services.tailscale.extraSetFlags = [ "--operator=lodestar-server" ];

    # The dashboard is reached over the tailnet only.
    networking.firewall.interfaces.tailscale0.allowedTCPPorts = [ cfg.listenPort ];

    environment.etc."lodestar-server/hosts.json".text = builtins.toJSON (
      lib.mapAttrsToList (name: h: {
        inherit name;
        inherit (h)
          system
          user
          channel
          os
          ;
      }) cfg.hosts
    );
    environment.etc."lodestar-server/config.json".text = builtins.toJSON (
      {
        listen = "${cfg.listenAddress}:${toString cfg.listenPort}";
        agent_port = cfg.agentPort;
        hosts_file = "/etc/lodestar-server/hosts.json";
        repo_url = cfg.repoUrl;
        # Only used by promote; without it nothing is ever pushed.
        push_url = if cfg.pushUrl != null then cfg.pushUrl else cfg.repoUrl;
        deploy_key_file = cfg.deployKeyFile;
        state_dir = stateDir;
        promote = if cfg.promote == null then null else { inherit (cfg.promote) from to; };
        access =
          if cfg.testing.disableAccess then
            { kind = "disabled"; }
          else
            {
              kind = "tailscale";
              allowed_users = cfg.allowedUsers;
              allowed_tags = cfg.allowedTags;
            };
        online =
          if cfg.testing.staticAddresses != null then
            {
              kind = "static";
              addresses = cfg.testing.staticAddresses;
            }
          else
            { kind = "tailscale"; };
        tailscale_bin = lib.getExe' config.services.tailscale.package "tailscale";
      }
      // lib.optionalAttrs (cfg.testing.pollSecs != null) { poll_secs = cfg.testing.pollSecs; }
    );

    systemd.services.lodestar-server = {
      description = "Lodestar server";
      wantedBy = [ "multi-user.target" ];
      after = [
        "tailscaled.service"
        "network-online.target"
      ];
      wants = [ "network-online.target" ];
      environment.HOME = stateDir;
      path = [
        pkgs.git
        pkgs.openssh
        config.services.tailscale.package
      ];
      restartTriggers = [
        config.environment.etc."lodestar-server/config.json".source
        config.environment.etc."lodestar-server/hosts.json".source
      ];
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} serve --config ${configPath}";
        User = "lodestar-server";
        Group = "lodestar-server";
        StateDirectory = "lodestar-server";
        # The tailnet address may not exist yet at boot: retry until it does.
        Restart = "on-failure";
        RestartSec = 5;
      };
    };
  };
}
