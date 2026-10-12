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

      linuxPkgs = nixpkgs.legacyPackages.x86_64-linux;

      # Evaluation-time check: a minimal NixOS system with both modules enabled opens
      # the agent and dashboard ports on tailscale0 (and only there).
      firewallCheck =
        let
          system = lib.nixosSystem {
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
                };
              }
            ];
          };
          inherit (system.config.networking.firewall) interfaces allowedTCPPorts;
          ports = interfaces.tailscale0.allowedTCPPorts;
        in
        assert lib.assertMsg (
          builtins.elem 7461 ports && builtins.elem 8080 ports
        ) "the modules must open 7461 and 8080 on tailscale0 (has ${builtins.toJSON ports})";
        assert lib.assertMsg (
          !(builtins.elem 7461 allowedTCPPorts) && !(builtins.elem 8080 allowedTCPPorts)
        ) "the modules must not open 7461/8080 on every interface";
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
        inherit server;
        default = {
          imports = [
            agentNixos
            server
          ];
        };
      };

      darwinModules = {
        agent = agentDarwin;
        default = agentDarwin;
      };

      lib.hostsFrom = import ./nix/lib/hostsFrom.nix { inherit lib; };

      checks =
        lib.recursiveUpdate
          (forEachSystem (
            { built, ... }:
            {
              bindings = built.bindingsCheck;
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
