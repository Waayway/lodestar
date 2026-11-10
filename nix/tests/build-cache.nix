# End-to-end NixOS VM test of the build-once feature: the server's build gate builds a
# commit on the builder over a real ssh-ng connection, publishes it there with
# `ssh lodestar@builder cache-publish …` into a signed file cache that nginx serves, and
# the client's agent applies it by substituting from `http://builder/nix` (its only
# substituter) instead of building. Also: a second apply of the same commit builds
# nothing; with the cache down the client builds itself after `connect-timeout`; a commit
# that fails to build is refused with 409 and never reaches the agent.
#
# The fleet repository is `build-cache-flake.nix` (no nixpkgs; derivations built with a
# static busybox in the sandbox). Keys (SSH and cache signing) are generated in the test.
# Needs an x86_64-linux builder with KVM.
{ pkgs, modules }:
let
  inherit (pkgs) lib;
  user = "alice";
  repoUrl = "git://manager/infra.git";
  cacheDir = "/srv/nix-cache";
  stateDir = "/var/lib/lodestar-server";
  sshKey = "${stateDir}/builder-key";
  tokenFile = "/run/lodestar-build-cache-token";
  testToken = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

  # Builds of the test flake use it as `/bin/busybox` (same path on builder and client,
  # so both evaluate the same derivations).
  sandboxBusybox = "/bin/busybox=${pkgs.pkgsStatic.busybox}/bin/busybox";

  # The test flake's switch-to-configuration writes the commit here; the agent reports it.
  fakeRevision = pkgs.writeShellScript "fake-revision" ''
    ${pkgs.coreutils}/bin/cat /var/lib/lodestar-agent/fake-revision 2>/dev/null || true
  '';
in
pkgs.testers.runNixOSTest {
  name = "lodestar-build-cache";

  nodes = {
    # The Lodestar server with the build gate, and the git remote of the fleet repository.
    manager =
      { nodes, ... }:
      {
        imports = modules;
        services.gitDaemon = {
          enable = true;
          basePath = "/srv/git";
          exportAll = true;
        };
        systemd.tmpfiles.rules = [
          "d /srv/git 0755 root root -"
          "f ${tokenFile} 0400 lodestar-server lodestar-server - ${testToken}"
        ];
        programs.git = {
          enable = true;
          config.safe.directory = "*";
        };
        networking.firewall.allowedTCPPorts = [ 9418 ];
        environment.systemPackages = [
          pkgs.jq
          pkgs.openssh
        ];
        nix.settings.experimental-features = [
          "nix-command"
          "flakes"
        ];

        services.lodestar-server = {
          enable = true;
          inherit repoUrl;
          pushUrl = repoUrl;
          agentTokenFiles.client = tokenFile;
          hosts.client = {
            inherit user;
            system = "x86_64-linux";
            channel = "unstable";
            os = "linux";
          };
          listenAddress = "127.0.0.1";
          builds = {
            enable = true;
            store = "ssh-ng://lodestar@builder";
            sshKeyFile = sshKey;
            timeoutSecs = 600;
          };
          testing = {
            disableAccess = true;
            pollSecs = 5;
            staticAddresses.client = nodes.client.networking.primaryIPAddress;
          };
        };
        # Started by the test once the repository and the builder's key are in place (a
        # prebuild at start would otherwise record a failure for the head).
        systemd.services.lodestar-server.wantedBy = lib.mkForce [ ];
      };

    # The cache host: builds over ssh-ng, publishes and serves the file cache.
    builder =
      { nodes, ... }:
      {
        imports = modules;
        services.lodestar-cache = {
          enable = true;
          inherit cacheDir;
          signingKeyFile = "/var/lib/cache-key/key";
          allowedClients = [
            nodes.client.networking.primaryIPAddress
            nodes.client.networking.primaryIPv6Address
          ];
        };
        services.openssh.enable = true;
        # The test's stand-ins for the NFS mount and the opnix secret.
        systemd.tmpfiles.rules = [
          "d ${cacheDir} 0755 lodestar users -"
          "d /var/lib/cache-key 0700 lodestar users -"
        ];
        nix.settings = {
          experimental-features = [
            "nix-command"
            "flakes"
          ];
          extra-sandbox-paths = [ sandboxBusybox ];
        };
        virtualisation.memorySize = 1024;
      };

    # A fleet host: its agent builds its system from its checkout, substituting from the
    # cache only (no cache.nixos.org; the test has no network anyway).
    client = {
      imports = modules;
      services.lodestar-agent = {
        enable = true;
        inherit user repoUrl;
        tokenFile = tokenFile;
        channel = "unstable";
        managerHost = "manager";
        listen = "0.0.0.0:7461";
        revisionCommand = [ "${fakeRevision}" ];
      };
      users.users.${user}.isNormalUser = true;
      systemd.tmpfiles.rules = [ "f ${tokenFile} 0400 root root - ${testToken}" ];
      environment.systemPackages = [
        pkgs.git
        pkgs.curl
        pkgs.jq
      ];
      # TEST ONLY: as in vm.nix, the agent port on the test network (no tailscale0).
      networking.firewall.interfaces.eth1.allowedTCPPorts = [ 7461 ];
      nix.settings = {
        experimental-features = [
          "nix-command"
          "flakes"
        ];
        substituters = lib.mkForce [ "http://builder/nix" ];
        connect-timeout = 5;
        # Without it a narinfo query that times out (cache or router down) fails the
        # whole build instead of building locally; the "cache down" subtest shows it.
        fallback = true;
        extra-sandbox-paths = [ sandboxBusybox ];
      };
      # The cache's public key, generated in the test.
      nix.extraOptions = "!include /etc/nix/test-cache-key.conf";
    };
  };

  testScript = ''
    import json
    import shlex
    import time

    def commit(change):
        """Commits `change` (a shell snippet run in the work tree) to unstable and pushes it."""
        manager.succeed(
            "set -e; cd /tmp/work; "
            "export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t; "
            f"{change}; git add -A; git commit -q -m change; git push -q origin unstable"
        )
        return manager.succeed("git -C /tmp/work rev-parse unstable").strip()

    def apply_channel():
        out = manager.succeed(
            "curl -s --max-time 900 -w '\\n%{http_code}' -X POST -H 'X-Lodestar: 1' "
            "localhost:8080/api/channels/unstable/apply"
        )
        body, code = out.strip().rsplit("\n", 1)
        return int(code), body

    def applies_on_client():
        return client.succeed("ls /var/lib/lodestar-agent/applies/ 2>/dev/null | grep -c '\\.json$' || true").strip()

    def wait_applied(apply_id, timeout):
        record = f"/var/lib/lodestar-agent/applies/{apply_id}.json"
        client.wait_until_succeeds(f"jq -e '.finished_at != null' {record}", timeout=timeout)
        state = json.loads(client.succeed(f"cat {record}"))
        log = client.succeed(f"cat /var/lib/lodestar-agent/applies/{apply_id}.log")
        print(f"apply {apply_id}: {state}\n--- log ---\n{log}--- end ---")
        assert state["state"] == "ok", state
        return log

    def result_file(c):
        return f"${stateDir}/builds/{c}.json"

    def ssh_logins():
        """sshd's "Accepted publickey" lines for the lodestar user on the builder."""
        return int(builder.succeed(
            "journalctl -b --no-pager | grep -c 'Accepted publickey for lodestar' || true"
        ).strip())

    start_all()

    # The fleet repository: unstable with one commit.
    manager.wait_for_unit("git-daemon.service")
    manager.succeed("""
      set -e
      export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
      git init -q -b unstable /tmp/work
      cd /tmp/work
      cp ${./build-cache-flake.nix} flake.nix
      chmod 644 flake.nix
      echo one > marker.txt
      git add -A; git commit -q -m c1
      git clone -q --bare /tmp/work /srv/git/infra.git
      git remote add origin /srv/git/infra.git
    """)
    head1 = manager.succeed("git -C /tmp/work rev-parse unstable").strip()

    # The cache signing key (as in cache-publish.nix) and the client trusting it.
    builder.wait_for_unit("sshd.service")
    builder.wait_for_unit("nginx.service")
    builder.succeed("su - lodestar -c 'nix key generate-secret --key-name builder-test-1 > /var/lib/cache-key/key'")
    public_key = builder.succeed("su - lodestar -c 'nix key convert-secret-to-public < /var/lib/cache-key/key'").strip()
    client.succeed(f"echo 'extra-trusted-public-keys = {public_key}' > /etc/nix/test-cache-key.conf")
    client.succeed("systemctl restart nix-daemon.socket nix-daemon.service")
    assert client.succeed("nix config show connect-timeout").strip() == "5"
    assert client.succeed("nix config show fallback").strip() == "true"
    assert public_key in client.succeed("nix config show trusted-public-keys").split()
    assert client.succeed("nix config show substituters").strip() == "http://builder/nix"

    # The server's SSH key to the builder; the builder lets it in as `lodestar`.
    manager.succeed(
        "install -d -m 700 -o lodestar-server -g lodestar-server ${stateDir}",
        "runuser -u lodestar-server -- ssh-keygen -q -t ed25519 -N ''' -C lodestar-builder-test -f ${sshKey}",
    )
    builder_pub = manager.succeed("cat ${sshKey}.pub").strip()
    builder.succeed(
        "install -d -m 700 -o lodestar -g users /home/lodestar/.ssh",
        f"echo {shlex.quote(builder_pub)} > /home/lodestar/.ssh/authorized_keys",
        "chown lodestar:users /home/lodestar/.ssh/authorized_keys",
        "chmod 600 /home/lodestar/.ssh/authorized_keys",
    )

    with subtest("a non-interactive ssh as the server finds the builder's commands"):
        # The way the server runs `cache-publish` (and ssh-ng runs `nix-daemon --stdio`).
        found = manager.succeed(
            "runuser -u lodestar-server -- ssh -i ${sshKey} -o BatchMode=yes "
            "-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null "
            "lodestar@builder 'command -v cache-publish nix-daemon'"
        ).split()
        assert len(found) == 2, found

    # The client's checkout.
    client.wait_for_unit("lodestar-agent.service")
    client.wait_for_open_port(7461)
    client.succeed("su - ${user} -c 'git clone -q -b unstable ${repoUrl} ~/.flake'")

    manager.succeed("systemctl start lodestar-server.service")
    manager.wait_for_open_port(8080)
    manager.wait_until_succeeds(
        "curl -sf localhost:8080/api/snapshot | jq -e '.hosts[] | select(.name==\"client\") | .online == \"on\"'",
        timeout=60,
    )

    with subtest("the gate builds the head on the builder and publishes it"):
        code, body = apply_channel()
        assert code == 202, (code, body)
        apply1 = json.loads(body)["apply_ids"]["client"]
        result = json.loads(manager.succeed(f"cat {result_file(head1)}"))
        print(f"gate result: {result}")
        host = result["hosts"]["client"]
        assert host["ok"], result
        toplevel = host["path"]
        assert toplevel.endswith("-nixos-system-client"), toplevel
        # Built on the builder (ultimate there), never on the manager.
        manager.fail(f"test -e {toplevel}")
        info = json.loads(builder.succeed(f"nix path-info --json {toplevel}"))
        assert info[toplevel]["ultimate"], info
        builds = json.loads(manager.succeed("curl -sf localhost:8080/api/builds"))
        unstable = next(c for c in builds if c["name"] == "unstable")
        assert unstable["build"]["state"] == "ok" and unstable["build"]["commit"] == head1, unstable
        # Published: the system and the marker package it references, signed.
        # The roots of the commit are the published paths: only those built on the builder.
        marker = builder.succeed(f"cat {toplevel}/marker").strip()
        roots = builder.succeed(f"cat ${cacheDir}/.roots/{head1}").split()
        assert sorted(roots) == sorted([toplevel, marker]), roots
        for path in [toplevel, marker]:
            path_hash = path.removeprefix("/nix/store/").split("-")[0]
            builder.succeed(f"test -f ${cacheDir}/{path_hash}.narinfo")
        builder.succeed(
            f"nix store verify --store file://${cacheDir} --trusted-public-keys '{public_key}' {toplevel} {marker}"
        )

    with subtest("nginx serves the cache and hides its dotfiles"):
        client.succeed("curl -sf http://builder/nix/nix-cache-info | grep -qx 'Priority: 50'")
        code = client.succeed("curl -s -o /dev/null -w '%{http_code}' http://builder/nix/.roots/").strip()
        assert code == "404", code

    with subtest("the client substitutes its system instead of building it"):
        log = wait_applied(apply1, timeout=300)
        for path in [marker, toplevel]:
            assert f"copying path '{path}' from 'http://builder/nix'" in log, path
        assert "building '" not in log, "the client built something"
        client.succeed(f"test \"$(readlink -f /nix/var/nix/profiles/system)\" = {toplevel}")
        manager.wait_until_succeeds(
            f"curl -sf localhost:8080/api/snapshot | jq -e '.hosts[] | select(.name==\"client\") | .running == \"{head1}\"'",
            timeout=60,
        )

    with subtest("a second apply of the same commit builds nothing"):
        # The gate answers from its result file: the file is not rewritten, and the server
        # does not even connect to the builder (sshd logs every login; a build or a publish
        # would be at least one).
        mtime = manager.succeed(f"stat -c %Y.%y {result_file(head1)}")
        logins = ssh_logins()
        assert logins > 0, logins
        code, body = apply_channel()
        assert code == 202, (code, body)
        wait_applied(json.loads(body)["apply_ids"]["client"], timeout=120)
        assert manager.succeed(f"stat -c %Y.%y {result_file(head1)}") == mtime
        assert ssh_logins() == logins, (ssh_logins(), logins)

    with subtest("cache down: the client builds itself after connect-timeout"):
        head2 = commit("echo two > marker.txt")
        builder.succeed("systemctl stop nginx.service")
        # Like a host that is down: the port drops packets instead of refusing them.
        builder.succeed("iptables -I INPUT -p tcp --dport 80 -j DROP")
        builder.succeed("ip6tables -I INPUT -p tcp --dport 80 -j DROP")
        client.fail("curl -s --max-time 10 http://builder/nix/nix-cache-info")
        code, body = apply_channel()
        assert code == 202, (code, body)
        started = time.monotonic()
        log = wait_applied(json.loads(body)["apply_ids"]["client"], timeout=300)
        took = time.monotonic() - started
        print(f"apply with the cache down took {took:.0f}s")
        assert "building '" in log and "-lodestar-test-marker.drv'" in log, "no local build"
        assert "from 'http://builder/nix'" not in log, "substituted although the cache is down"
        assert took < 180, took
        builder.succeed("iptables -D INPUT -p tcp --dport 80 -j DROP")
        builder.succeed("ip6tables -D INPUT -p tcp --dport 80 -j DROP")
        builder.succeed("systemctl start nginx.service")

    with subtest("a commit that fails to build is refused and never reaches the agent"):
        head3 = commit("touch fail")
        before = applies_on_client()
        code, body = apply_channel()
        print(f"apply of a failing commit: HTTP {code}: {body}")
        assert code == 409, (code, body)
        message = json.loads(body)["error"]
        assert message == f"build of {head3[:7]} failed on the builder: client", message
        result = json.loads(manager.succeed(f"cat {result_file(head3)}"))
        assert not result["hosts"]["client"]["ok"], result
        assert "deliberate build failure" in result["hosts"]["client"]["log_tail"], result
        builds = json.loads(manager.succeed("curl -sf localhost:8080/api/builds"))
        unstable = next(c for c in builds if c["name"] == "unstable")
        assert unstable["build"]["state"] == "failed" and unstable["build"]["failed_hosts"] == ["client"], unstable
        time.sleep(2)
        assert applies_on_client() == before, (applies_on_client(), before)
        audit = json.loads(manager.succeed("curl -sf 'localhost:8080/api/audit?limit=5'"))
        assert any(e["kind"] == "apply-refused" for e in audit), audit
  '';
}
