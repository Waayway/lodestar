# End-to-end NixOS VM test of Lodestar: a git remote, the server (with an agent for
# itself) and two laptops, configured only through the modules' options.
{ pkgs, modules }:
let
  user = "alice";
  repoUrl = "git://remote/infra.git";

  # The fake rebuild records the commit it was asked to apply; the fake revision
  # command reports it back, so "running" follows the last apply.
  revFile = "/var/lib/lodestar-agent/fake-revision";
  fakeRebuild = pkgs.writeShellScript "fake-rebuild" ''
    echo "$2" > ${revFile}
  '';
  fakeRevision = pkgs.writeShellScript "fake-revision" ''
    ${pkgs.coreutils}/bin/cat ${revFile} 2>/dev/null || true
  '';

  agentNode = {
    imports = modules;
    services.lodestar-agent = {
      enable = true;
      inherit user repoUrl;
      channel = "unstable";
      managerHost = "manager";
      listen = "0.0.0.0:7461";
      rebuildCommand = [ "${fakeRebuild}" ];
      revisionCommand = [ "${fakeRevision}" ];
    };
    users.users.${user}.isNormalUser = true;
    environment.systemPackages = [ pkgs.git ];
    # TEST ONLY: the module opens the agent port on tailscale0, which these VMs
    # lack; open it on the test network instead. (checks.x86_64-linux.firewall
    # asserts the module opens it on tailscale0.)
    networking.firewall.interfaces.eth1.allowedTCPPorts = [ 7461 ];
  };
in
pkgs.testers.runNixOSTest {
  name = "lodestar-vm";

  nodes = {
    remote =
      { ... }:
      {
        services.gitDaemon = {
          enable = true;
          basePath = "/srv/git";
          exportAll = true;
        };
        systemd.tmpfiles.rules = [ "d /srv/git 0755 root root -" ];
        environment.systemPackages = [ pkgs.git ];
        programs.git = {
          enable = true;
          config.safe.directory = "*";
        };
        networking.firewall.allowedTCPPorts = [ 9418 ];
      };

    manager =
      { nodes, ... }:
      {
        imports = [ agentNode ];
        environment.systemPackages = [ pkgs.jq ];
        # TEST ONLY: as for the agent port, the dashboard port is opened on tailscale0
        # by the module; there is no tailscale0 here.
        networking.firewall.interfaces.eth1.allowedTCPPorts = [ 8080 ];
        services.lodestar-server = {
          enable = true;
          inherit repoUrl;
          hosts = pkgs.lib.genAttrs [ "manager" "laptop1" "laptop2" ] (_: {
            inherit user;
            system = "x86_64-linux";
            channel = "unstable";
            os = "linux";
          });
          pushUrl = repoUrl;
          listenAddress = "0.0.0.0";
          testing = {
            disableAccess = true;
            pollSecs = 5;
            staticAddresses = {
              # Connecting to the host's own eth1 address is sourced from that address,
              # which the manager's agent accepts ("manager" resolves to it).
              manager = nodes.manager.networking.primaryIPAddress;
              laptop1 = nodes.laptop1.networking.primaryIPAddress;
              laptop2 = nodes.laptop2.networking.primaryIPAddress;
            };
          };
        };
      };

    laptop1 = agentNode;
    laptop2 = agentNode;
  };

  testScript = ''
    import json

    agents = [manager, laptop1, laptop2]

    def snapshot_host(name):
        out = manager.succeed("curl -sf localhost:8080/api/snapshot")
        return next(h for h in json.loads(out)["hosts"] if h["name"] == name)

    def apply(name):
        out = manager.succeed(
            f"curl -s -w '\\n%{{http_code}}' -X POST -H 'X-Lodestar: 1' localhost:8080/api/hosts/{name}/apply"
        )
        body, code = out.strip().rsplit("\n", 1)
        assert code == "202", f"apply {name}: HTTP {code}: {body}"
        return json.loads(body)

    start_all()

    # Git remote: unstable is one commit ahead of stable.
    remote.wait_for_unit("git-daemon.service")
    remote.succeed("""
      set -e
      export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
      git init -q -b unstable /tmp/work
      cd /tmp/work
      echo one > flake.nix; git add flake.nix; git commit -q -m c0
      git branch stable
      echo two > flake.nix; git commit -q -am c1
      git clone -q --bare /tmp/work /srv/git/infra.git
      git remote add origin /srv/git/infra.git
    """)
    head1 = remote.succeed("git -C /tmp/work rev-parse unstable").strip()

    # 1. Units are up; every agent user has a checkout one commit behind unstable.
    manager.wait_for_unit("lodestar-server.service")
    manager.wait_for_open_port(8080)
    for m in agents:
        m.wait_for_unit("lodestar-agent.service")
        m.wait_for_open_port(7461)
        m.succeed("su - alice -c 'git clone -q -b unstable git://remote/infra.git ~/.flake && git -C ~/.flake reset -q --hard HEAD~1'")

    # The manager's own agent is reachable through the server.
    manager.wait_until_succeeds(
        "curl -sf localhost:8080/api/snapshot | jq -e '.hosts[] | select(.name==\"manager\") | .online == \"on\" and .checkout != null'"
    )

    # 2-3. Apply laptop1: accepted, then it runs the head.
    res = apply("laptop1")
    assert res["agent_status"] in ("202", "200"), res
    manager.wait_until_succeeds(
        f"curl -sf localhost:8080/api/snapshot | jq -e '.hosts[] | select(.name==\"laptop1\") | .running == \"{head1}\"'"
    )
    assert snapshot_host("laptop1")["checkout"]["head"] == head1

    # 4. A dirty laptop2 skips the apply and keeps its HEAD.
    # (git as alice: root would refuse the repo as "dubious ownership")
    before = laptop2.succeed("su - alice -c 'git -C ~/.flake rev-parse HEAD'").strip()
    laptop2.succeed("echo local >> /home/alice/.flake/flake.nix")
    l2_id = apply("laptop2")["apply_id"]
    manager.wait_until_succeeds(
        f"curl -sf localhost:8080/api/snapshot | jq -e '.hosts[] | select(.name==\"laptop2\") | .last_apply | .apply_id == \"{l2_id}\" and .skip_reason == \"dirty\"'"
    )
    assert laptop2.succeed("su - alice -c 'git -C ~/.flake rev-parse HEAD'").strip() == before
    assert snapshot_host("laptop2")["running"] != head1

    # 5. Source check: laptop1 is not the manager, so laptop2's agent refuses it.
    code = laptop1.succeed("curl -4 -s -o /dev/null -w '%{http_code}' laptop2:7461/status").strip()
    assert code == "403", f"expected 403 from laptop2, got {code}"

    # 6. Agent down: the apply cannot be delivered. After the agent is back, the
    # server's poller re-sends it within two poll intervals (poll_secs = 5).
    laptop1.systemctl("stop lodestar-agent.service")
    remote.succeed("""
      set -e
      export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
      cd /tmp/work
      echo three > flake.nix; git commit -q -am c2
      git push -q origin unstable
    """)
    head2 = remote.succeed("git -C /tmp/work rev-parse unstable").strip()
    assert head2 != head1
    res = apply("laptop1")
    assert res["agent_status"] == "unreachable", res
    laptop1.systemctl("start lodestar-agent.service")
    manager.wait_until_succeeds(
        f"curl -sf localhost:8080/api/snapshot | jq -e '.hosts[] | select(.name==\"laptop1\") | .running == \"{head2}\"'",
        timeout=30,
    )
  '';
}
