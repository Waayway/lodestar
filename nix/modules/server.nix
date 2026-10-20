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

  proxyEnabled = cfg.proxyListen.address != null;
  isIpv6 = lib.hasInfix ":";
  ipv4Proxies = lib.filter (a: !isIpv6 a) cfg.trustedProxies;
  ipv6Proxies = lib.filter isIpv6 cfg.trustedProxies;
  proxyPort = toString cfg.proxyListen.port;
  # Socket address: IPv6 needs brackets.
  proxySocket =
    if isIpv6 cfg.proxyListen.address then
      "[${cfg.proxyListen.address}]:${proxyPort}"
    else
      "${cfg.proxyListen.address}:${proxyPort}";

  linkType = import ../lib/linkType.nix { inherit lib; } { hostType = types.str; };

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

    links = mkOption {
      type = types.attrsOf linkType;
      default = { };
      example = lib.literalExpression ''
        lodestar.lib.linksFrom { inherit (self) nixosConfigurations darwinConfigurations; }
      '';
      description = ''
        Service links shown in the Services tab, by id. Usually computed with
        `lodestar.lib.linksFrom` from every configuration's `lodestar.links`.
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

    proxyListen = {
      address = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "10.0.10.170";
        description = ''
          LAN address of the router entrance: a second listener that only the
          `trustedProxies` may reach, for the public URL. Null (default) turns it off and
          leaves the dashboard on the tailnet only.
        '';
      };
      port = mkOption {
        type = types.port;
        default = 8081;
        description = "TCP port of the router entrance.";
      };
    };

    trustedProxies = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = [ "10.0.10.110" ];
      description = ''
        Addresses of the reverse proxies allowed to reach the router entrance; the firewall
        opens `proxyListen.port` for these sources only, and the server refuses every other
        peer. Required when `proxyListen.address` is set.
      '';
    };

    publicUrl = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "https://lodestar.example.com";
      description = "Public URL of the dashboard, `https://host[:port]` (the OIDC redirect is `<publicUrl>/auth/callback`). Plain http is refused at startup: the session cookie is `__Host-…; Secure`.";
    };

    oidc = {
      issuer = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "https://id.example.com/oauth2/openid/lodestar";
        description = "OpenID Connect issuer URL used for login on the router entrance.";
      };
      clientId = mkOption {
        type = types.str;
        default = "lodestar";
        description = "OAuth2 client id.";
      };
      clientSecretFile = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = ''
          File with the OAuth2 client secret, outside the Nix store and readable by the
          `lodestar-server` user, from a secrets module such as opnix
          (`services.onepassword-secrets.secrets.<name> = { reference = "op://…";
          owner = "lodestar-server"; mode = "0400"; };` and
          `clientSecretFile = config.services.onepassword-secrets.secretPaths.<name>;`).
        '';
      };
      readerGroup = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "lodestar-users@example.com";
        description = "Group (exact `groups` claim value) whose members may read the dashboard.";
      };
      adminGroup = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "admins@example.com";
        description = "Group (exact `groups` claim value) whose members may act; wins over `readerGroup`.";
      };
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
      {
        assertion = !proxyEnabled || cfg.trustedProxies != [ ];
        message = "services.lodestar-server.trustedProxies must not be empty while proxyListen.address is set.";
      }
      {
        assertion =
          !proxyEnabled
          || (
            cfg.publicUrl != null
            && cfg.oidc.issuer != null
            && cfg.oidc.clientSecretFile != null
            && cfg.oidc.readerGroup != null
            && cfg.oidc.adminGroup != null
          );
        message = "services.lodestar-server: publicUrl, oidc.issuer, oidc.clientSecretFile, oidc.readerGroup and oidc.adminGroup must be set while proxyListen.address is set.";
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

    # The router entrance is open to the trusted proxies only, never to every source.
    networking.firewall.extraInputRules = lib.mkIf (proxyEnabled && config.networking.nftables.enable) (
      lib.concatStringsSep "\n" (
        lib.optional (
          ipv4Proxies != [ ]
        ) "ip saddr { ${lib.concatStringsSep ", " ipv4Proxies} } tcp dport ${proxyPort} accept"
        ++ lib.optional (
          ipv6Proxies != [ ]
        ) "ip6 saddr { ${lib.concatStringsSep ", " ipv6Proxies} } tcp dport ${proxyPort} accept"
      )
    );
    networking.firewall.extraCommands = lib.mkIf (proxyEnabled && !config.networking.nftables.enable) (
      lib.concatStringsSep "\n" (
        map (a: "iptables -A nixos-fw -p tcp -s ${a} --dport ${proxyPort} -j nixos-fw-accept") ipv4Proxies
        ++ map (
          a: "ip6tables -A nixos-fw -p tcp -s ${a} --dport ${proxyPort} -j nixos-fw-accept"
        ) ipv6Proxies
      )
    );

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
    environment.etc."lodestar-server/links.json".text = builtins.toJSON (
      lib.mapAttrsToList (id: l: {
        inherit id;
        inherit (l)
          title
          url
          group
          description
          icon
          host
          ;
        check = {
          inherit (l.check) enable url;
          expect_status = l.check.expectStatus;
          insecure_tls = l.check.insecureTls;
        };
      }) cfg.links
    );
    environment.etc."lodestar-server/config.json".text = builtins.toJSON (
      {
        listen = "${cfg.listenAddress}:${toString cfg.listenPort}";
        agent_port = cfg.agentPort;
        hosts_file = "/etc/lodestar-server/hosts.json";
        links_file = "/etc/lodestar-server/links.json";
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
      // lib.optionalAttrs proxyEnabled {
        proxy = {
          listen = proxySocket;
          trusted_proxies = cfg.trustedProxies;
          public_url = cfg.publicUrl;
          oidc = {
            inherit (cfg.oidc) issuer;
            client_id = cfg.oidc.clientId;
            client_secret_file = cfg.oidc.clientSecretFile;
            reader_group = cfg.oidc.readerGroup;
            admin_group = cfg.oidc.adminGroup;
          };
        };
      }
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
        config.environment.etc."lodestar-server/links.json".source
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
