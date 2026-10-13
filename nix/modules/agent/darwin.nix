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
  signing = cfg.macos.stableSigning;

  # Installs the private key for `certificate` from stdin, after checking it matches.
  signingKeyTool = pkgs.writeShellApplication {
    name = "lodestar-agent-signing-key";
    runtimeInputs = [
      pkgs.coreutils
      (lib.getBin pkgs.openssl)
    ];
    text = ''
      if [ "$(id -u)" != 0 ]; then
        echo "usage: sudo lodestar-agent-signing-key < key.pem" >&2
        exit 1
      fi
      dir=${lib.escapeShellArg signing.directory}
      umask 077
      mkdir -p "$dir"
      chmod 0700 "$dir"
      cat > "$dir/key.pem.new"
      if [ "$(openssl x509 -in ${pkgs.writeText "lodestar-agent-signing.pem" signing.certificate} -noout -pubkey)" != \
        "$(openssl pkey -in "$dir/key.pem.new" -pubout 2>/dev/null)" ]; then
        rm -f "$dir/key.pem.new"
        echo "lodestar-agent-signing-key: this key does not belong to the configured certificate" >&2
        exit 1
      fi
      mv -f "$dir/key.pem.new" "$dir/key.pem"
      echo "installed $dir/key.pem; switch (or activate) to sign the agent with it" >&2
    '';
  };
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

      certificate = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = lib.literalExpression "builtins.readFile ./lodestar-agent-signing.pem";
        description = ''
          PEM certificate to sign with on every Mac, so all of them share one code
          identity. Its private key is not in the configuration: place it once per Mac
          with `sudo lodestar-agent-signing-key < key.pem` (it lands in `directory`).
          Until then the agent stays ad-hoc signed and activation warns. Null: each Mac
          creates its own key and certificate on first activation.
        '';
      };

      directory = mkOption {
        type = types.str;
        default = "/var/lib/lodestar-agent/signing";
        description = ''
          Root-only (0700) directory with the signing key and certificate (`key.pem`,
          `cert.pem`, `cert.sha1`). Without `certificate` they are created on first
          activation; remove the directory and switch to get a new certificate (macOS
          asks for the permissions once more).
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
    environment.systemPackages = lib.mkIf (signing.enable && signing.certificate != null) [
      signingKeyTool
    ];

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
          inherit (cfg.macos.stableSigning) certificate;
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
