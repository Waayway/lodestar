# Evaluation check of the agent modules without nix-darwin: the darwin module is
# evaluated against stub declarations of the few nix-darwin options it sets. Asserts
# that launchd and the agent config use binaryPath, that signing and the firewall rule
# follow their options, and shellchecks the generated activation snippets (darwin with
# and without signing, NixOS).
{
  pkgs,
  lib,
  darwinModule,
}:
let
  inherit (lib) mkOption types;

  stubs = {
    options = {
      assertions = mkOption {
        type = types.listOf types.unspecified;
        default = [ ];
      };
      warnings = mkOption {
        type = types.listOf types.str;
        default = [ ];
      };
      networking.hostName = mkOption {
        type = types.nullOr types.str;
        default = null;
      };
      environment.systemPackages = mkOption {
        type = types.listOf types.package;
        default = [ ];
      };
      environment.variables = mkOption {
        type = types.attrsOf types.str;
        default = { };
      };
      environment.etc = mkOption {
        type = types.attrsOf (types.submodule { options.text = mkOption { type = types.lines; }; });
        default = { };
      };
      launchd.daemons = mkOption {
        type = types.attrsOf (
          types.submodule { options.serviceConfig = mkOption { type = types.attrsOf types.anything; }; }
        );
        default = { };
      };
      system.activationScripts = mkOption {
        type = types.attrsOf (
          types.submodule {
            options.text = mkOption {
              type = types.lines;
              default = "";
            };
          }
        );
        default = { };
      };
    };
  };

  # The package is only a store path in the snippet; a stand-in keeps the check from
  # building the agent.
  fakeAgent = pkgs.writeShellScriptBin "lodestar-agent" "exit 0";

  evalDarwin =
    extra:
    (lib.evalModules {
      modules = [
        stubs
        darwinModule
        {
          _module.args.pkgs = pkgs;
          networking.hostName = "laptop";
          services.lodestar-agent = {
            enable = true;
            user = "alice";
            managerHost = "manager";
            repoUrl = "https://example.com/infra.git";
            package = fakeAgent;
            cliPackage = fakeAgent;
          };
        }
        extra
      ];
    }).config;

  signed = evalDarwin { };
  unsigned = evalDarwin {
    services.lodestar-agent.macos.stableSigning.enable = false;
    services.lodestar-agent.macos.firewall.enable = false;
  };
  custom = evalDarwin { services.lodestar-agent.binaryPath = "/opt/lodestar/agent"; };

  programOf =
    config: builtins.head config.launchd.daemons.lodestar-agent.serviceConfig.ProgramArguments;
  agentExeOf =
    config: (builtins.fromJSON config.environment.etc."lodestar-agent/config.json".text).agent_exe;
  scriptOf = config: config.system.activationScripts.extraActivation.text;
  has = needle: haystack: lib.hasInfix needle haystack;

  linuxSnippet = import ../modules/agent/install-binary.nix {
    inherit pkgs lib;
    source = "${fakeAgent}/bin/lodestar-agent";
    binaryPath = "/var/lib/lodestar-agent/bin/lodestar-agent";
  };

  checks = [
    {
      ok = programOf signed == "/var/lib/lodestar-agent/bin/lodestar-agent";
      what = "launchd runs the default binaryPath (got ${programOf signed})";
    }
    {
      ok = agentExeOf signed == "/var/lib/lodestar-agent/bin/lodestar-agent";
      what = "agent_exe is binaryPath";
    }
    {
      ok = programOf custom == "/opt/lodestar/agent" && agentExeOf custom == "/opt/lodestar/agent";
      what = "a custom binaryPath reaches launchd and agent_exe";
    }
    {
      ok = programOf unsigned == "/var/lib/lodestar-agent/bin/lodestar-agent";
      what = "without signing launchd still runs binaryPath";
    }
    {
      ok =
        signed.launchd.daemons.lodestar-agent.serviceConfig.EnvironmentVariables.LODESTAR_PACKAGE
        == "${fakeAgent}";
      what = "the plist changes with the package (reload trigger)";
    }
    {
      ok =
        has "/usr/bin/codesign --force" (scriptOf signed)
        && has "org.lodestar.agent" (scriptOf signed)
        && has "socketfilterfw --add" (scriptOf signed);
      what = "stableSigning and firewall on: the snippet signs and allows the binary";
    }
    {
      ok =
        !(has "codesign" (scriptOf unsigned))
        && !(has "socketfilterfw" (scriptOf unsigned))
        && has "/var/lib/lodestar-agent/bin/lodestar-agent" (scriptOf unsigned);
      what = "stableSigning and firewall off: the snippet only copies";
    }
  ];

  failed = builtins.filter (check: !check.ok) checks;

  snippet =
    name: text:
    pkgs.writeText "lodestar-${name}.sh" ''
      #!/usr/bin/env bash
      # shellcheck disable=SC2034
      ${builtins.unsafeDiscardStringContext text}
    '';
in
assert lib.assertMsg (failed == [ ])
  "agent module checks failed: ${lib.concatMapStringsSep "; " (check: check.what) failed}";
pkgs.runCommand "lodestar-agent-module"
  {
    nativeBuildInputs = [ pkgs.shellcheck ];
  }
  ''
    shellcheck -s bash ${snippet "darwin-signed" (scriptOf signed)}
    shellcheck -s bash ${snippet "darwin-unsigned" (scriptOf unsigned)}
    shellcheck -s bash ${snippet "nixos" linuxSnippet}
    touch $out
  ''
