# Evaluation check of the software-version options: a NixOS system (evaluated, never
# built, so it runs on any host system) with the agent's `versions` and the server's
# `updates` and `metrics` set. Asserts the generated versions.json, the `updates` and
# `metrics` parts of the server's config.json, the agent's `versions_file`, and that the
# input-name validation fires.
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
  bad = mkSystem {
    services.lodestar-server.updates.software."Bad Name" = {
      input = "nix pkgs";
      upstream.github = "not-a-repo";
    };
  };
  failedBad = lib.filter (a: !a.assertion) bad.config.assertions;

  checks = [
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
      ok = lib.length failedBad == 3;
      what = "bad software name, input and github fail assertions (got ${toString (lib.length failedBad)})";
    }
  ];

  failed = lib.filter (check: !check.ok) checks;
in
assert lib.assertMsg (failed == [ ])
  "updates module checks failed: ${lib.concatMapStringsSep "; " (check: check.what) failed}";
pkgs.runCommand "lodestar-updates-module" { } "touch $out"
