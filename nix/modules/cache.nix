# The cache host role: the machine the Lodestar server builds on over ssh-ng and whose
# file binary cache every Linux host substitutes from. Sets up the `lodestar` user the
# server logs in as (a trusted Nix user with `cache-publish` and `cache-prune` on its
# PATH), a daily `cache-prune`, and nginx serving `cacheDir` at `/nix/` to
# `allowedClients` only. NixOS only; exported as `nixosModules.cache`.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.lodestar-cache;
  inherit (lib) mkOption types;

  scripts = pkgs.callPackage ../packages/cache-scripts.nix {
    nix = config.nix.package;
    inherit (cfg) cacheDir signingKeyFile;
  };

  port = toString cfg.listen.port;
  isIpv6 = lib.hasInfix ":";
  ipv4Clients = lib.filter (a: !isIpv6 a) cfg.allowedClients;
  ipv6Clients = lib.filter isIpv6 cfg.allowedClients;
  rule =
    command: action: address:
    "${command} -w ${action} nixos-fw -p tcp -s ${address} --dport ${port} -j nixos-fw-accept";
  rules =
    action: map (rule "iptables" action) ipv4Clients ++ map (rule "ip6tables" action) ipv6Clients;
in
{
  options.services.lodestar-cache = {
    enable = lib.mkEnableOption "the Lodestar build cache host (builds over ssh-ng, publishes a file binary cache)";

    user = mkOption {
      type = types.str;
      default = "lodestar";
      description = "Account the Lodestar server logs in as (ssh-ng builds and `cache-publish`). A trusted Nix user.";
    };

    authorizedKeys = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = [ "ssh-ed25519 AAAA… lodestar-builder" ];
      description = "SSH public keys allowed to log in as `user` (the server's `builds.sshKeyFile`).";
    };

    cacheDir = mkOption {
      type = types.strMatching "/[A-Za-z0-9._/-]+";
      default = "/mnt/nix-cache";
      description = ''
        The file binary cache (`nix-cache-info`, `*.narinfo`, `nar/`, and the private
        `.roots/`). Not created by the module: mount it (e.g. NFS) writable by `user`.
      '';
    };

    signingKeyFile = mkOption {
      type = types.strMatching "/[A-Za-z0-9._/-]+";
      example = "/run/secrets/nix-cache-signing-key";
      description = "Secret key (`nix key generate-secret`) that signs published paths; readable by `user`.";
    };

    listen.port = mkOption {
      type = types.port;
      default = 80;
      description = "Port nginx serves the cache on (`http://<host>:<port>/nix`).";
    };

    allowedClients = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = [ "10.0.10.110" ];
      description = "Addresses allowed to reach `listen.port` (e.g. the reverse proxy). The port is never opened to anyone else.";
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.${cfg.user} = {
      # ssh-ng runs `nix-daemon --stdio` and the server runs `cache-publish`: a real shell.
      isNormalUser = true;
      shell = pkgs.bashInteractive;
      hashedPassword = "!";
      openssh.authorizedKeys.keys = cfg.authorizedKeys;
      packages = [ scripts ];
    };

    # Remote builds of input-addressed derivations need a trusted user.
    nix.settings.trusted-users = [ cfg.user ];

    systemd.services.lodestar-cache-prune = {
      description = "Delete build cache paths no channel keeps";
      unitConfig.RequiresMountsFor = [ cfg.cacheDir ];
      serviceConfig = {
        Type = "oneshot";
        User = cfg.user;
        ExecStart = "${scripts}/bin/cache-prune ${cfg.cacheDir}";
      };
    };
    systemd.timers.lodestar-cache-prune = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = "daily";
        Persistent = true;
        RandomizedDelaySec = "1h";
      };
    };

    services.nginx = {
      enable = true;
      virtualHosts.lodestar-cache = {
        default = true;
        listen = [
          {
            addr = "0.0.0.0";
            port = cfg.listen.port;
          }
          {
            addr = "[::]";
            port = cfg.listen.port;
          }
        ];
        locations."/nix/" = {
          alias = "${cfg.cacheDir}/";
          extraConfig = ''
            autoindex off;
            disable_symlinks on;
            # .roots/, .lock and temporary files are private.
            location ~ /\. {
              return 404;
            }
            # A narinfo changes when a path is republished or pruned; NARs never do.
            location ~ \.narinfo$ {
              add_header Cache-Control "no-cache";
            }
          '';
        };
      };
    };

    # The port is open to allowedClients only, never to every source.
    networking.firewall.extraCommands = lib.mkIf (!config.networking.nftables.enable) (
      lib.concatStringsSep "\n" (rules "-A")
    );
    networking.firewall.extraStopCommands = lib.mkIf (!config.networking.nftables.enable) (
      lib.concatMapStringsSep "\n" (r: "${r} || true") (rules "-D")
    );
    networking.firewall.extraInputRules = lib.mkIf config.networking.nftables.enable (
      lib.concatStringsSep "\n" (
        lib.optional (
          ipv4Clients != [ ]
        ) "ip saddr { ${lib.concatStringsSep ", " ipv4Clients} } tcp dport ${port} accept"
        ++ lib.optional (
          ipv6Clients != [ ]
        ) "ip6 saddr { ${lib.concatStringsSep ", " ipv6Clients} } tcp dport ${port} accept"
      )
    );
  };
}
