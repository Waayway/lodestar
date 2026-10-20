{
  description = "Lodestar: see and steer which commit of your Nix flake every machine runs";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
    }:
    let
      inherit (nixpkgs) lib;

      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "x86_64-linux"
        "aarch64-linux"
      ];

      # Calls `f { pkgs, built }` for every system: { <system> = f ...; }.
      forEachSystem =
        f:
        lib.genAttrs systems (
          system:
          let
            pkgs = nixpkgs.legacyPackages.${system};
          in
          f {
            inherit pkgs;
            built = import ./default.nix { inherit pkgs crane; };
          }
        );

      # The modules default their packages to this flake's build for the host's system.
      packagesFor = system: self.packages.${system};

      agentNixos = import ./nix/modules/agent/nixos.nix { inherit packagesFor; };
      agentDarwin = import ./nix/modules/agent/darwin.nix { inherit packagesFor; };
      server = import ./nix/modules/server.nix { inherit packagesFor; };
      links = import ./nix/modules/links.nix { inherit lib; };

      linuxPkgs = nixpkgs.legacyPackages.x86_64-linux;

      # Evaluation-time check: a minimal NixOS system with both modules enabled opens
      # the agent and dashboard ports on tailscale0 (and only there), and runs the agent
      # from binaryPath.
      firewallCheck =
        let
          mkSystem =
            serverExtra:
            lib.nixosSystem {
              system = "x86_64-linux";
              modules = [
                self.nixosModules.default
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
                  };
                  services.lodestar-server = {
                    enable = true;
                    repoUrl = "https://example.com/infra.git";
                    pushUrl = "git@example.com:infra.git";
                    listenAddress = "100.64.0.1";
                  }
                  // serverExtra;
                }
              ];
            };
          system = mkSystem { };
          proxyOptions = {
            proxyListen.address = "10.0.10.170";
            trustedProxies = [ "10.0.10.110" ];
            publicUrl = "https://lodestar.example.com";
            oidc = {
              issuer = "https://id.example.com/oauth2/openid/lodestar";
              clientSecretFile = "/run/secrets/lodestar-oidc";
              readerGroup = "lodestar-users@example.com";
              adminGroup = "admins@example.com";
            };
          };
          proxied = mkSystem proxyOptions;
          incomplete = mkSystem { proxyListen.address = "10.0.10.170"; };
          proxiedFirewall = proxied.config.networking.firewall;
          proxiedConfig = builtins.fromJSON (
            builtins.unsafeDiscardStringContext
              proxied.config.environment.etc."lodestar-server/config.json".text
          );
          serverConfig = builtins.fromJSON (
            builtins.unsafeDiscardStringContext system.config.environment.etc."lodestar-server/config.json".text
          );
          failedMessages = map (a: a.message) (lib.filter (a: !a.assertion) incomplete.config.assertions);
          inherit (system.config.networking.firewall) interfaces allowedTCPPorts;
          ports = interfaces.tailscale0.allowedTCPPorts;
          execStart = system.config.systemd.services.lodestar-agent.serviceConfig.ExecStart;
          agentConfig = builtins.fromJSON system.config.environment.etc."lodestar-agent/config.json".text;
        in
        assert lib.assertMsg (
          builtins.elem 7461 ports && builtins.elem 8080 ports
        ) "the modules must open 7461 and 8080 on tailscale0 (has ${builtins.toJSON ports})";
        assert lib.assertMsg (
          !(builtins.elem 7461 allowedTCPPorts) && !(builtins.elem 8080 allowedTCPPorts)
        ) "the modules must not open 7461/8080 on every interface";
        assert lib.assertMsg (lib.hasPrefix "/var/lib/lodestar-agent/bin/lodestar-agent " execStart)
          "the agent unit must run binaryPath (ExecStart = ${execStart})";
        assert lib.assertMsg (
          agentConfig.agent_exe == "/var/lib/lodestar-agent/bin/lodestar-agent"
        ) "agent_exe must be binaryPath";
        assert lib.assertMsg (
          !(builtins.elem 8081 proxiedFirewall.allowedTCPPorts)
          && !(builtins.elem 8081 proxiedFirewall.interfaces.tailscale0.allowedTCPPorts)
        ) "the router entrance port must not be open to every source or on tailscale0";
        assert lib.assertMsg
          (lib.hasInfix "-p tcp -s 10.0.10.110 --dport 8081 -j nixos-fw-accept" proxiedFirewall.extraCommands)
          "the router entrance port must be opened for the trusted proxy only (extraCommands = ${proxiedFirewall.extraCommands})";
        assert lib.assertMsg (
          proxiedConfig.proxy.trusted_proxies == [ "10.0.10.110" ]
          && proxiedConfig.proxy.oidc.client_id == "lodestar"
        ) "config.json must carry the proxy section";
        assert lib.assertMsg (
          !(serverConfig ? proxy) || serverConfig.proxy == null
        ) "config.json must have no proxy section without proxyListen.address";
        assert lib.assertMsg (lib.any (lib.hasInfix "trustedProxies") failedMessages)
          "proxyListen.address without trustedProxies must fail an assertion naming trustedProxies";
        linuxPkgs.runCommand "lodestar-firewall" { } "touch $out";
    in
    {
      packages = forEachSystem (
        { built, ... }:
        {
          inherit (built)
            lodestar-agent
            lodestar-server
            lodestar-web
            lodestar
            ;
          default = built.lodestar;
        }
      );

      apps = forEachSystem (
        { pkgs, built }:
        rec {
          lodestar = {
            type = "app";
            program = lib.getExe built.lodestar;
            meta.description = "CLI for a Lodestar server (status, rebuild, log, promote, audit)";
          };
          default = lodestar;
          # `nix run .#lodestar-demo`: the dashboard against a simulated set of hosts (needs git).
          lodestar-demo = {
            type = "app";
            program = "${pkgs.writeShellScript "lodestar-demo" ''
              export PATH=${pkgs.git}/bin:$PATH
              exec ${lib.getExe built.lodestar-server} demo "$@"
            ''}";
            meta.description = "Lodestar dashboard demo against simulated hosts";
          };
        }
      );

      nixosModules = {
        agent = agentNixos;
        inherit server links;
        default = {
          imports = [
            agentNixos
            server
            links
          ];
        };
      };

      darwinModules = {
        agent = agentDarwin;
        inherit links;
        default = {
          imports = [
            agentDarwin
            links
          ];
        };
      };

      lib.hostsFrom = import ./nix/lib/hostsFrom.nix { inherit lib; };
      lib.linksFrom = import ./nix/lib/linksFrom.nix { inherit lib; };

      checks =
        lib.recursiveUpdate
          (forEachSystem (
            { pkgs, built }:
            {
              bindings = built.bindingsCheck;
              # The darwin module (with stubs for nix-darwin) and the binary install snippets.
              agent-module = import ./nix/tests/agent-module.nix {
                inherit pkgs lib;
                darwinModule = agentDarwin;
              };
              links-from = import ./nix/tests/links-from.nix {
                inherit pkgs lib;
                linksFrom = self.lib.linksFrom;
              };
            }
          ))
          {
            x86_64-linux.firewall = firewallCheck;
            # Server, two agents and a git remote in VMs. Needs an x86_64-linux builder with KVM.
            x86_64-linux.vm = import ./nix/tests/vm.nix {
              pkgs = linuxPkgs;
              modules = [ self.nixosModules.default ];
            };
          };

      devShells = forEachSystem (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              clippy
              rustfmt
              nodejs_22
              sqlite
              git
            ];
          };
        }
      );

      formatter = forEachSystem ({ pkgs, ... }: pkgs.nixfmt-tree);
    };
}
