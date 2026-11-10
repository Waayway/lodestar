# Options and platform-independent config of the Lodestar agent, shared by the NixOS
# (systemd) and nix-darwin (launchd) modules. Writes the agent's JSON config to
# /etc/lodestar-agent/config.json and installs the `lodestar` CLI.
#
# `packagesFor` maps a system to this flake's packages; `isDarwin` picks the platform
# defaults (decided by which module the consumer imports, not by `pkgs`, so evaluating
# the option tree never depends on `config`).
{ packagesFor, isDarwin }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.lodestar-agent;
  inherit (lib) mkOption types;
in
{
  options.services.lodestar-agent = {
    enable = lib.mkEnableOption "the Lodestar agent, which applies channel commits to this host";

    user = mkOption {
      type = types.str;
      example = "alice";
      description = ''
        Account that owns the flake checkout. git and `nix build` run as this user; only
        the switch runs as root.
      '';
    };

    hostName = mkOption {
      type = types.str;
      # nix-darwin's networking.hostName may be null; the assertion below reports that.
      default = if config.networking.hostName == null then "" else config.networking.hostName;
      defaultText = lib.literalExpression "config.networking.hostName";
      description = ''
        Name of this host as the server knows it (a key of
        `services.lodestar-server.hosts`). Also used in the default `flakeAttr`.
      '';
    };

    channel = mkOption {
      type = types.str;
      default = "main";
      example = "stable";
      description = "Branch of the repository this host follows.";
    };

    managerHost = mkOption {
      type = types.str;
      example = "manager";
      description = ''
        Tailscale (MagicDNS) name of the host running `lodestar-server`. The agent only
        accepts requests from the addresses this name resolves to, and only with the
        server's token (`tokenFile`).
      '';
    };

    tokenFile = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = lib.literalExpression "config.sops.secrets.lodestar-agent-token.path";
      description = ''
        Required. File with the token the server must present on every request
        (`Authorization: Bearer <token>`), in addition to coming from `managerHost`. A
        path outside the Nix store, owned by root with mode 0400 or 0600 (the agent refuses
        a file group or others may read), from a secrets module such as sops-nix, agenix
        or opnix. One token per line, 32 random bytes as 64 lowercase hex digits
        (`openssl rand -hex 32`) or 43 unpadded base64url characters (only the format is
        checked, not the randomness); not a symlink, in a directory group and others cannot write (unless sticky);
        every listed token is accepted, so a rotation can list the old and the new one for
        a while. Read on every request: a rotation needs no restart. The same token goes
        in the server's `services.lodestar-server.agentTokenFiles.<hostName>`.
      '';
    };

    repoUrl = mkOption {
      type = types.str;
      example = "https://github.com/alice/infra";
      description = "Git URL of the flake repository the checkout tracks as `origin`.";
    };

    flakePath = mkOption {
      type = types.str;
      default =
        if isDarwin || !(config.users.users ? ${cfg.user}) then
          "/Users/${cfg.user}/.flake"
        else
          "${config.users.users.${cfg.user}.home}/.flake";
      defaultText =
        if isDarwin then
          lib.literalExpression ''"/Users/''${user}/.flake"''
        else
          lib.literalExpression ''"''${config.users.users.''${user}.home}/.flake"'';
      description = "The user's checkout of the flake repository on this host.";
    };

    flakeAttr = mkOption {
      type = types.str;
      default =
        if isDarwin then
          "darwinConfigurations.${cfg.hostName}.system"
        else
          "nixosConfigurations.${cfg.hostName}.config.system.build.toplevel";
      defaultText =
        if isDarwin then
          lib.literalExpression ''"darwinConfigurations.''${hostName}.system"''
        else
          lib.literalExpression ''"nixosConfigurations.''${hostName}.config.system.build.toplevel"'';
      description = "Flake output the agent builds (`nix build <flakePath>#<flakeAttr>`) and switches to.";
    };

    port = mkOption {
      type = types.port;
      default = 7461;
      description = "TCP port the agent listens on.";
    };

    listen = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "0.0.0.0:7461";
      description = "Explicit `ip:port` to bind; null binds the host's Tailscale addresses.";
    };

    rebuildCommand = mkOption {
      type = types.nullOr (types.listOf types.str);
      default = null;
      description = "TEST ONLY: replaces build and switch; called with the host name and the commit.";
    };

    revisionCommand = mkOption {
      type = types.nullOr (types.listOf types.str);
      default = null;
      description = "TEST ONLY: replaces the running-revision command (`nixos-version`/`darwin-version`).";
    };

    binaryPath = mkOption {
      type = types.str;
      default = "/var/lib/lodestar-agent/bin/lodestar-agent";
      description = ''
        Fixed path the agent binary is copied to on activation (only when `package`
        changed). The daemon and the apply wrapper run from here, so the program path
        stays the same across updates; on macOS this copy is also what gets signed (see
        `macos.stableSigning`). Its directory is made root-owned, mode 0755.
      '';
    };

    package = mkOption {
      type = types.package;
      default = (packagesFor pkgs.stdenv.hostPlatform.system).lodestar-agent;
      defaultText = lib.literalExpression "lodestar.packages.\${pkgs.stdenv.hostPlatform.system}.lodestar-agent";
      description = "The lodestar-agent package.";
    };

    cliPackage = mkOption {
      type = types.package;
      default = (packagesFor pkgs.stdenv.hostPlatform.system).lodestar;
      defaultText = lib.literalExpression "lodestar.packages.\${pkgs.stdenv.hostPlatform.system}.lodestar";
      description = "The `lodestar` CLI package.";
    };

    installCli = mkOption {
      type = types.bool;
      default = true;
      description = ''
        Install the `lodestar` CLI on this host and point it at the server
        (`LODESTAR_SERVER`, see `serverUrl`).
      '';
    };

    serverUrl = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "http://manager:8080";
      description = ''
        URL of the Lodestar server for the `lodestar` CLI, exported as `LODESTAR_SERVER`
        when `installCli` is set. Null means `http://<managerHost>:<port>`, where the port
        is this host's `services.lodestar-server.listenPort` when it runs the server, else
        8080; set it when the server listens on another port.
      '';
    };

    versions = mkOption {
      type = types.attrsOf types.str;
      default = { };
      example = lib.literalExpression "{ tailscale = config.services.tailscale.package.version; }";
      description = ''
        Versions of software on this host that the server tracks, by software name
        (the keys of `services.lodestar-server.updates.software`). Written to
        `/etc/lodestar/versions.json`, which the agent reports in its status.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.hostName != "";
        message = "services.lodestar-agent.hostName is empty: set networking.hostName or services.lodestar-agent.hostName.";
      }
    ]
    ++ [
      {
        assertion =
          cfg.tokenFile != null
          && lib.hasPrefix "/" cfg.tokenFile
          && !(lib.hasPrefix builtins.storeDir cfg.tokenFile);
        message = "services.lodestar-agent.tokenFile must be set to an absolute path outside the Nix store (a secret file with the token the server presents); the agent refuses requests without it.";
      }
    ]
    ++ [
      {
        assertion = lib.hasPrefix "/" cfg.binaryPath && !(lib.hasSuffix "/" cfg.binaryPath);
        message = "services.lodestar-agent.binaryPath must be an absolute file path.";
      }
    ]
    ++ lib.optional (!isDarwin) {
      assertion = config.users.users ? ${cfg.user};
      message = "services.lodestar-agent.user = \"${cfg.user}\" is not a user of this system (users.users.${cfg.user}); it must own the flake checkout.";
    };

    environment.systemPackages = lib.optional cfg.installCli cfg.cliPackage;
    environment.variables = lib.mkIf cfg.installCli {
      LODESTAR_SERVER =
        if cfg.serverUrl != null then
          cfg.serverUrl
        else
          let
            server = config.services.lodestar-server or { };
            port = if server.enable or false then server.listenPort else 8080;
          in
          "http://${cfg.managerHost}:${toString port}";
    };
    environment.etc."lodestar/versions.json".text = builtins.toJSON cfg.versions;
    environment.etc."lodestar-agent/config.json".text = builtins.toJSON {
      host = cfg.hostName;
      inherit (cfg)
        user
        channel
        port
        listen
        ;
      flake_path = cfg.flakePath;
      flake_attr = cfg.flakeAttr;
      repo_url = cfg.repoUrl;
      manager_host = cfg.managerHost;
      state_dir = "/var/lib/lodestar-agent";
      rebuild_command = cfg.rebuildCommand;
      revision_command = cfg.revisionCommand;
      launcher = if isDarwin then "detach" else "systemd_run";
      os = if isDarwin then "darwin" else "linux";
      agent_exe = cfg.binaryPath;
      versions_file = "/etc/lodestar/versions.json";
      # The path only: the token itself never enters the Nix store.
      token_file = cfg.tokenFile;
    };
  };
}
