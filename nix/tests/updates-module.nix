# Evaluation check of the software-version options: a NixOS system (evaluated, never
# built, so it runs on any host system) with the agent's `versions` and the server's
# `updates` and `metrics` set. Asserts the generated versions.json, the `updates` and
# `metrics` parts of the server's config.json, the agent's `versions_file`, and that the
# input-name validation fires. The agent token: tokenFile reaches the agent's config.json,
# agentTokenFiles the server's, and missing or store-path token files fail assertions. Also the cache host role (`services.lodestar-cache`): off by
# default, and when on its user, nginx location and allowedClients-only firewall rules.
{
  pkgs,
  lib,
  nixosModule,
}:
let
  mkSystem =
    extra:
    lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        nixosModule
        {
          system.stateVersion = "26.05";
          boot.loader.grub.enable = false;
          fileSystems."/" = {
            device = "/dev/sda1";
            fsType = "ext4";
          };
          networking.hostName = "manager";
          users.users.alice.isNormalUser = true;
          services.tailscale.enable = true;
          services.lodestar-agent = {
            enable = true;
            user = "alice";
            managerHost = "manager";
            repoUrl = "https://example.com/infra.git";
            versions.tailscale = "1.104.2";
            tokenFile = "/run/secrets/lodestar-agent-token";
          };
          services.lodestar-server = {
            enable = true;
            repoUrl = "https://example.com/infra.git";
            pushUrl = "git@example.com:infra.git";
            listenAddress = "100.64.0.1";
            updates.software.tailscale = {
              input = "nixpkgs-unstable";
              upstream.github = "tailscale/tailscale";
            };
          };
        }
        extra
      ];
    };

  etcText =
    system: name: builtins.unsafeDiscardStringContext system.config.environment.etc.${name}.text;
  serverJson = system: builtins.fromJSON (etcText system "lodestar-server/config.json");

  plain = mkSystem { };
  withMetrics = mkSystem { services.lodestar-server.metrics.address = "10.0.10.170"; };
  withMetricsV6 = mkSystem {
    services.lodestar-server.metrics = {
      address = "fd00::1";
      port = 9000;
    };
  };
  builds = {
    enable = true;
    store = "ssh-ng://lodestar@10.0.10.171";
    sshKeyFile = "/run/keys/builder";
  };
  withBuilds = mkSystem { services.lodestar-server.builds = builds; };
  buildsFail =
    extra:
    failedOf (mkSystem {
      services.lodestar-server.builds = builds // extra;
    }) != [ ];
  withCache = mkSystem {
    services.lodestar-cache = {
      enable = true;
      authorizedKeys = [ "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample lodestar-builder" ];
      signingKeyFile = "/run/secrets/nix-cache-signing-key";
      allowedClients = [
        "10.0.10.110"
        "fd00::110"
      ];
    };
  };
  cacheUser = withCache.config.users.users.lodestar;
  cacheFirewall = withCache.config.networking.firewall;
  cacheHost = withCache.config.services.nginx.virtualHosts.lodestar-cache;
  cacheLocation = cacheHost.locations."/nix/";
  cachePrune = withCache.config.systemd.services.lodestar-cache-prune;
  bad = mkSystem {
    services.lodestar-server.updates.software."Bad Name" = {
      input = "nix pkgs";
      upstream.github = "not-a-repo";
    };
  };
  failedBad = lib.filter (a: !a.assertion) bad.config.assertions;
  failedOf = system: lib.filter (a: !a.assertion) system.config.assertions;
  # Whether exactly the assertions mentioning `needle` fail.
  failsOn =
    needle: extra:
    let
      failures = failedOf (mkSystem extra);
    in
    failures != [ ] && lib.all (failure: lib.hasInfix needle failure.message) failures;
  withHost = {
    services.lodestar-server.hosts.box = {
      channel = "main";
      os = "linux";
      system = "x86_64-linux";
      user = "alice";
    };
  };
  channelFails =
    extra:
    let
      failures = failedOf (mkSystem extra);
    in
    lib.length failures == 1 && lib.hasInfix "updates.channel" (lib.head failures).message;

  checks = [
    {
      ok =
        (serverJson plain).builds == {
          enable = false;
          store = "";
          publish_command = [ "cache-publish" ];
          timeout_secs = 7200;
        };
      what = "config.json carries builds off by default (no ssh_key_file)";
    }
    {
      ok =
        (serverJson withBuilds).builds == {
          enable = true;
          store = "ssh-ng://lodestar@10.0.10.171";
          ssh_key_file = "/run/keys/builder";
          publish_command = [ "cache-publish" ];
          timeout_secs = 7200;
        };
      what = "config.json carries the builds options in snake_case";
    }
    {
      ok =
        lib.all
          (
            store:
            failedOf (mkSystem {
              services.lodestar-server.builds = builds // {
                inherit store;
              };
            }) == [ ]
          )
          [
            "ssh-ng://lodestar@10.0.10.171"
            "ssh://builder"
            "ssh-ng://lodestar@cache.example:2222"
          ];
      what = "user@host[:port] stores pass";
    }
    {
      ok = failedOf withBuilds == [ ];
      what = "a complete enabled builds config passes";
    }
    {
      ok = lib.all buildsFail [
        { store = ""; }
        { store = "https://cache"; }
        { store = "ssh-ng://-oProxyCommand=x"; }
        { store = "ssh-ng://"; }
        { store = "ssh-ng://lodestar@host name"; }
        { store = "ssh-ng://host?remote-program=x"; }
        { store = "ssh-ng://host/path"; }
        { publishCommand = [ ]; }
        { publishCommand = [ "" ]; }
      ];
      what = "enabled builds with an empty or non-ssh store or empty publishCommand fail an assertion";
    }
    {
      ok =
        failedOf (mkSystem {
          services.lodestar-server.builds = {
            store = "";
          };
        }) == [ ];
      what = "disabled builds are not validated";
    }
    {
      ok = etcText plain "lodestar/versions.json" == builtins.toJSON { tailscale = "1.104.2"; };
      what = "the agent writes versions.json from `versions`";
    }
    {
      ok =
        (builtins.fromJSON (etcText plain "lodestar-agent/config.json")).versions_file
        == "/etc/lodestar/versions.json";
      what = "the agent config names versions_file";
    }
    {
      ok =
        (builtins.fromJSON (etcText plain "lodestar-agent/config.json")).token_file
        == "/run/secrets/lodestar-agent-token";
      what = "the agent config names token_file (the path only, never the token)";
    }
    {
      ok = failsOn "tokenFile" { services.lodestar-agent.tokenFile = lib.mkForce null; };
      what = "an agent without tokenFile fails an assertion";
    }
    {
      ok = failsOn "tokenFile" {
        services.lodestar-agent.tokenFile = lib.mkForce "${pkgs.writeText "token" "x"}";
      };
      what = "an agent tokenFile in the Nix store fails an assertion";
    }
    {
      ok = failsOn "agentTokenFiles" withHost;
      what = "a managed host without an agent token file fails an assertion";
    }
    {
      ok = failsOn "agentTokenFiles" (
        lib.recursiveUpdate withHost {
          services.lodestar-server.agentTokenFiles.box = "${pkgs.writeText "token" "x"}";
        }
      );
      what = "an agentTokenFiles entry in the Nix store fails an assertion";
    }
    {
      ok =
        let
          system = mkSystem (
            lib.recursiveUpdate withHost {
              services.lodestar-server.agentTokenFiles.box = "/run/secrets/lodestar-agent-box";
            }
          );
        in
        failedOf system == [ ]
        && (serverJson system).agent_token_files == { box = "/run/secrets/lodestar-agent-box"; };
      what = "agentTokenFiles reaches config.json as agent_token_files";
    }
    {
      ok =
        (serverJson plain).updates == {
          channel = "unstable";
          interval_secs = 86400;
          upstream_interval_secs = 21600;
          software.tailscale = {
            input = "nixpkgs-unstable";
            upstream.github = "tailscale/tailscale";
          };
        };
      what = "config.json carries updates with the defaults";
    }
    {
      ok = lib.hasSuffix "/bin/nix" (serverJson plain).nix_bin;
      what = "config.json carries nix_bin";
    }
    {
      ok = !((serverJson plain) ? metrics);
      what = "no metrics key while metrics.address is null";
    }
    {
      ok = (serverJson withMetrics).metrics == { listen = "10.0.10.170:9466"; };
      what = "metrics.listen is address:port";
    }
    {
      ok = (serverJson withMetricsV6).metrics == { listen = "[fd00::1]:9000"; };
      what = "an IPv6 metrics address is bracketed";
    }
    {
      ok =
        !(lib.elem 9466 plain.config.networking.firewall.allowedTCPPorts)
        && !(lib.elem 9466 withMetrics.config.networking.firewall.allowedTCPPorts)
        && !(lib.elem 9466 (
          withMetrics.config.networking.firewall.interfaces.tailscale0.allowedTCPPorts or [ ]
        ));
      what = "the module does not open the metrics port";
    }
    {
      ok = failedOf plain == [ ];
      what = "the default updates channel passes";
    }
    {
      ok = lib.all channelFails [
        { services.lodestar-server.updates.channel = "-x"; }
        { services.lodestar-server.updates.channel = "a..b"; }
        { services.lodestar-server.updates.channel = "un stable"; }
        { services.lodestar-server.updates.channel = ""; }
      ];
      what = "an invalid updates.channel fails an assertion";
    }
    {
      ok = lib.all channelFails [
        { services.lodestar-server.updates.channel = "stable"; }
        {
          services.lodestar-server.promote = {
            from = "unstable";
            to = "release";
          };
          services.lodestar-server.updates.channel = "release";
        }
        {
          services.lodestar-server.promote = null;
          services.lodestar-server.updates.channel = "stable";
        }
      ];
      what = "updates.channel equal to the promote target fails an assertion";
    }
    {
      ok =
        failedOf (mkSystem {
          services.lodestar-server.promote = {
            from = "unstable";
            to = "release";
          };
          services.lodestar-server.updates.channel = "stable";
        }) == [ ];
      what = "stable is allowed when it is not the promote target";
    }
    {
      ok =
        !plain.config.services.nginx.enable
        && !(plain.config.users.users ? lodestar)
        && !(plain.config.systemd.services ? lodestar-cache-prune);
      what = "the cache role is off by default";
    }
    {
      ok =
        cacheUser.isNormalUser
        && !cacheUser.isSystemUser
        && cacheUser.hashedPassword == "!"
        && lib.hasSuffix "/bin/bash" (lib.getExe cacheUser.shell)
        &&
          cacheUser.openssh.authorizedKeys.keys == [
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExample lodestar-builder"
          ]
        && lib.elem "lodestar" withCache.config.nix.settings.trusted-users
        && lib.any (p: p.name == "lodestar-cache-scripts") cacheUser.packages;
      what = "the cache user logs in with a key only, has a shell and the scripts, and is a trusted Nix user";
    }
    {
      ok =
        cacheLocation.alias == "/mnt/nix-cache/"
        && lib.hasInfix "location ~ /\\. {" cacheLocation.extraConfig
        && lib.hasInfix "return 404;" cacheLocation.extraConfig
        && lib.hasInfix ''add_header Cache-Control "no-cache";'' cacheLocation.extraConfig
        && lib.all (l: l.port == 80) cacheHost.listen;
      what = "nginx serves cacheDir at /nix/ on port 80, hides dotfiles, narinfos no-cache";
    }
    {
      ok =
        !(lib.elem 80 cacheFirewall.allowedTCPPorts)
        && lib.hasInfix "iptables -w -A nixos-fw -p tcp -s 10.0.10.110 --dport 80 -j nixos-fw-accept" cacheFirewall.extraCommands
        && lib.hasInfix "ip6tables -w -A nixos-fw -p tcp -s fd00::110 --dport 80 -j nixos-fw-accept" cacheFirewall.extraCommands
        &&
          lib.length (
            lib.filter (lib.hasInfix "--dport 80") (lib.splitString "\n" cacheFirewall.extraCommands)
          ) == 2
        && lib.hasInfix "iptables -w -D nixos-fw -p tcp -s 10.0.10.110 --dport 80 -j nixos-fw-accept || true" cacheFirewall.extraStopCommands;
      what = "the cache port is opened for allowedClients only";
    }
    {
      ok =
        cachePrune.serviceConfig.User == "lodestar"
        && lib.hasSuffix "/bin/cache-prune /mnt/nix-cache" cachePrune.serviceConfig.ExecStart
        && withCache.config.systemd.timers.lodestar-cache-prune.timerConfig.OnCalendar == "daily";
      what = "cache-prune runs daily as the cache user";
    }
    {
      ok = failedOf withCache == [ ];
      what = "an enabled cache role passes the assertions";
    }
    {
      ok = lib.length failedBad == 3;
      what = "bad software name, input and github fail assertions (got ${toString (lib.length failedBad)})";
    }
  ];

  failed = lib.filter (check: !check.ok) checks;
in
assert lib.assertMsg (failed == [ ])
  "updates module checks failed: ${lib.concatMapStringsSep "; " (check: check.what) failed}";
pkgs.runCommand "lodestar-updates-module" { } "touch $out"
