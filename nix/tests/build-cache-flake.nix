# The fleet repository's flake in the build-cache VM test (nix/tests/build-cache.nix),
# copied there as `flake.nix`. One host, `client`, whose "system" is two small derivations
# built with the static busybox the test puts in the build sandbox at `/bin/busybox`: no
# nixpkgs, no network. `marker.txt` (next to it in the repository) is the marker
# package's content; a file `fail` makes the system fail to build.
#
# The system has what the agent's apply uses: `bin/switch-to-configuration` (it records
# the commit for the test's revision command) and a reference to the marker package, so
# the client substitutes both paths from the cache or builds both itself.
{
  outputs =
    { self }:
    let
      busybox = "/bin/busybox";
      build =
        name: script: env:
        derivation (
          env
          // {
            inherit name;
            system = "x86_64-linux";
            builder = busybox;
            args = [
              "sh"
              "-c"
              script
            ];
          }
        );

      marker = build "lodestar-test-marker" ''echo "$text" > $out'' {
        text = builtins.readFile ./marker.txt;
      };

      toplevel =
        build "nixos-system-client"
          ''
            set -e
            ${busybox} mkdir -p $out/bin
            echo $marker > $out/marker
            printf '#!/bin/sh\necho %s > /var/lib/lodestar-agent/fake-revision\n' "$rev" \
              > $out/bin/switch-to-configuration
            ${busybox} chmod 755 $out/bin/switch-to-configuration
            ${if builtins.pathExists ./fail then "echo 'deliberate build failure' >&2; exit 1" else ""}
          ''
          {
            inherit marker;
            rev = self.rev or "dirty";
          };
    in
    {
      nixosConfigurations.client.config.system.build.toplevel = toplevel;
    };
}
