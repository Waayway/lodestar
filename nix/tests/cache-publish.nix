# NixOS VM test of the cache host role (nix/modules/cache.nix) with a real Nix store:
# builds a tiny derivation that references a nixpkgs path, publishes it with
# `cache-publish` as the `lodestar` user (login shell, as the server's ssh does) and a
# test key, and checks that the file cache holds only the path built here, signed so that
# `nix store verify` accepts it; that nginx serves the cache at /nix/ (narinfos
# `no-cache`) and hides `.roots/`; and that the prune service runs.
# Needs an x86_64-linux builder with KVM.
{ pkgs, modules }:
let
  cacheDir = "/srv/nix-cache";
  keyDir = "/var/lib/cache-test";
  commit = "1111111111111111111111111111111111111111";
  # A derivation built in the VM (so `ultimate`) whose output references hello (not).
  smoke = pkgs.writeText "smoke.nix" ''
    derivation {
      name = "smoke";
      system = builtins.currentSystem;
      builder = builtins.storePath "${pkgs.bash}" + "/bin/bash";
      args = [ "-c" "echo ''${builtins.storePath "${pkgs.hello}"} > $out" ];
    }
  '';
in
pkgs.testers.runNixOSTest {
  name = "lodestar-cache-publish";
  nodes.cache = {
    imports = modules;
    services.lodestar-cache = {
      enable = true;
      inherit cacheDir;
      signingKeyFile = "${keyDir}/key";
      allowedClients = [ "192.168.1.2" ];
    };
    # The test's stand-ins for the NFS mount and the opnix secret.
    systemd.tmpfiles.rules = [
      "d ${cacheDir} 0755 lodestar users -"
      "d ${keyDir} 0700 lodestar users -"
    ];
    nix.settings.experimental-features = [ "nix-command" ];
    system.extraDependencies = [
      pkgs.bash
      pkgs.hello
    ];
    environment.systemPackages = [ pkgs.curl ];
    virtualisation.memorySize = 1024;
  };

  testScript = ''
    import json
    import shlex

    cache.wait_for_unit("multi-user.target")
    cache.wait_for_unit("nginx.service")

    def as_lodestar(command):
        return cache.succeed(f"su - lodestar -c {shlex.quote(command)}")

    as_lodestar("nix key generate-secret --key-name test-1 > ${keyDir}/key")
    public_key = as_lodestar("nix key convert-secret-to-public < ${keyDir}/key").strip()

    smoke = as_lodestar("nix-build --no-out-link ${smoke}").strip()
    hello = "${pkgs.hello}"

    index = json.dumps({"stable": {"current": "${commit}", "previous": None}})
    as_lodestar(f"echo {shlex.quote(index)} | cache-publish ${commit} {smoke}")

    with subtest("only the path built here is published, signed"):
        smoke_hash = smoke.removeprefix("/nix/store/").split("-")[0]
        hello_hash = hello.removeprefix("/nix/store/").split("-")[0]
        cache.succeed(f"test -f ${cacheDir}/{smoke_hash}.narinfo")
        cache.fail(f"test -e ${cacheDir}/{hello_hash}.narinfo")
        cache.succeed("grep -qx 'Priority: 50' ${cacheDir}/nix-cache-info")
        info = json.loads(cache.succeed(f"nix path-info --store file://${cacheDir} --json {smoke}"))
        assert smoke in info and info[smoke] is not None, info
        assert hello in info[smoke]["references"], info
        cache.succeed(
            f"nix store verify --store file://${cacheDir} --trusted-public-keys '{public_key}' {smoke}"
        )
        roots = cache.succeed("cat ${cacheDir}/.roots/${commit}").split()
        assert roots == [smoke], roots

    with subtest("publishing again is a no-op and keeps the roots"):
        before = cache.succeed("ls -l --time-style=+%s ${cacheDir} ${cacheDir}/nar")
        as_lodestar(f"echo {shlex.quote(index)} | cache-publish ${commit} {smoke}")
        assert before == cache.succeed("ls -l --time-style=+%s ${cacheDir} ${cacheDir}/nar")

    with subtest("nginx serves the cache and hides .roots"):
        cache.succeed("curl -sf http://127.0.0.1/nix/nix-cache-info | grep -q 'StoreDir: /nix/store'")
        headers = cache.succeed(f"curl -sfI http://127.0.0.1/nix/{smoke_hash}.narinfo")
        assert "cache-control: no-cache" in headers.lower(), headers
        url = cache.succeed(f"sed -n 's/^URL: //p' ${cacheDir}/{smoke_hash}.narinfo").strip()
        cache.succeed(f"curl -sf -o /dev/null http://127.0.0.1/nix/{url}")
        for private in [".roots/channels.json", ".roots/${commit}", ".lock"]:
            code = cache.succeed(f"curl -s -o /dev/null -w '%{{http_code}}' http://127.0.0.1/nix/{private}")
            assert code == "404", f"{private}: {code}"

    with subtest("the prune service runs as the cache user"):
        cache.succeed("systemctl start lodestar-cache-prune.service")
        cache.succeed(f"test -f ${cacheDir}/{smoke_hash}.narinfo")
  '';
}
