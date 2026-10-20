# End-to-end NixOS VM test of Lodestar: a git remote, the server (with an agent for
# itself) and two laptops, configured only through the modules' options; plus a Kanidm
# server and a stand-in for the router that logs in through the router entrance.
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

  # Kanidm on https://acme.test with the snakeoil certificate nixpkgs' tests use (copied
  # to the store as nixpkgs does, for kanidm's mount namespace).
  certs = import "${pkgs.path}/nixos/tests/common/acme/server/snakeoil-certs.nix";
  kanidmDomain = certs.domain;
  certsPath = pkgs.runCommand "snakeoil-certs" { } ''
    mkdir $out
    cp ${certs.${kanidmDomain}.cert} $out/snakeoil.crt
    cp ${certs.${kanidmDomain}.key} $out/snakeoil.key
  '';
  kanidmPackage = pkgs.kanidmWithSecretProvisioning_1_11;
  publicUrl = "https://lodestar.example.test";
  # TEST ONLY: a fixed client secret, shared by Kanidm and Lodestar.
  clientSecretFile = pkgs.writeText "lodestar-client-secret" "very-strong-test-secret-for-lodestar";
  scopes = [
    "openid"
    "profile"
    "groups"
  ];

  # Logs in to the router entrance through Kanidm without a browser.
  kanidmLogin = pkgs.writers.writePython3Bin "kanidm-login" { } (builtins.readFile ./kanidm-login.py);

  # Trusts the snakeoil CA and finds Kanidm by its name.
  kanidmClient =
    { nodes, ... }:
    {
      security.pki.certificateFiles = [ certs.ca.cert ];
      networking.hosts.${nodes.kanidm.networking.primaryIPAddress} = [ kanidmDomain ];
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
      { config, nodes, ... }:
      {
        imports = [
          agentNode
          kanidmClient
        ];
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
          links = {
            # The agent answers on 7461 (its / is not a route: 404 counts as up here).
            agent = {
              url = "http://laptop1:7461/";
              host = "laptop1";
              group = "Fleet";
              check.expectStatus = [
                200
                404
              ];
            };
            closed = {
              url = "http://localhost:1/";
              host = "manager";
            };
          };
          listenAddress = "0.0.0.0";
          # The router entrance, for the router node only, with login through Kanidm.
          proxyListen.address = config.networking.primaryIPAddress;
          trustedProxies = [ nodes.router.networking.primaryIPAddress ];
          inherit publicUrl;
          oidc = {
            issuer = "https://${kanidmDomain}/oauth2/openid/lodestar";
            clientSecretFile = "${clientSecretFile}";
            readerGroup = "lodestar-users@${kanidmDomain}";
            adminGroup = "admins@${kanidmDomain}";
          };
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

    kanidm =
      { ... }:
      {
        services.kanidm = {
          package = kanidmPackage;
          server = {
            enable = true;
            settings = {
              origin = "https://${kanidmDomain}";
              domain = kanidmDomain;
              bindaddress = "[::]:443";
              tls_chain = "${certsPath}/snakeoil.crt";
              tls_key = "${certsPath}/snakeoil.key";
            };
          };
          provision = {
            enable = true;
            persons = {
              alice.displayName = "Alice";
              bob.displayName = "Bob";
              carol.displayName = "Carol";
            };
            groups = {
              lodestar-users.members = [ "alice" ];
              admins.members = [ "bob" ];
            };
            systems.oauth2.lodestar = {
              displayName = "Lodestar";
              originUrl = "${publicUrl}/auth/callback";
              originLanding = publicUrl;
              basicSecretFile = clientSecretFile;
              preferShortUsername = true;
              scopeMaps = {
                lodestar-users = scopes;
                admins = scopes;
              };
            };
          };
        };
        networking.firewall.allowedTCPPorts = [ 443 ];
        environment.systemPackages = [ pkgs.curl ];
      };

    # Stands in for the homelab router: the one trusted proxy of the router entrance.
    router =
      { ... }:
      {
        imports = [ kanidmClient ];
        environment.systemPackages = [
          kanidmLogin
          pkgs.curl
          pkgs.jq
        ];
      };
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
        # The agent runs from the root-owned copy at binaryPath, not from the store.
        assert m.succeed("stat -c '%U %a' /var/lib/lodestar-agent/bin /var/lib/lodestar-agent/bin/lodestar-agent").split() == ["root", "755", "root", "755"]
        pid = m.succeed("systemctl show -p MainPID --value lodestar-agent.service").strip()
        exe = m.succeed(f"readlink /proc/{pid}/exe").strip()
        assert exe == "/var/lib/lodestar-agent/bin/lodestar-agent", exe
        m.succeed("cmp /var/lib/lodestar-agent/bin/lodestar-agent \"$(cat /var/lib/lodestar-agent/bin/.lodestar-agent.source)\"")

    # The manager's own agent is reachable through the server.
    manager.wait_until_succeeds(
        "curl -sf localhost:8080/api/snapshot | jq -e '.hosts[] | select(.name==\"manager\") | .online == \"on\" and .checkout != null'"
    )

    # Service links: the file is written, the checker finds the agent up (and its host on)
    # and the closed port down with a refused connection.
    manager.succeed("test -f /etc/lodestar-server/links.json")
    manager.wait_until_succeeds(
        "curl -sf localhost:8080/api/snapshot | jq -e '"
        ".services[] | select(.id==\"agent\") | .check.state == \"up\" and .host_online == \"on\"'",
        timeout=60,
    )
    manager.wait_until_succeeds(
        "curl -sf localhost:8080/api/snapshot | jq -e '"
        ".services[] | select(.id==\"closed\") | .check.state == \"down\" and .check.reason.kind == \"connection_refused\"'",
        timeout=60,
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

    # 7. The router entrance, reached from the router with Kanidm login. Its IPv4
    # address: the firewall opens the port to the router's IPv4 address only, so a
    # connection to "manager" over IPv6 would be dropped.
    listen = manager.succeed("jq -r .proxy.listen /etc/lodestar-server/config.json").strip()
    entrance = f"http://{listen}"
    mark = "-H 'X-Homelab-Via: tailnet'"
    passwords = {
        "alice": "alice-very-strong-test-password",
        "bob": "bob-very-strong-test-password",
        "carol": "carol-very-strong-test-password",
    }

    def call(method, path, session=None, marked=False):
        headers = "-H 'X-Lodestar: 1'"
        if session:
            headers += f" -H 'Cookie: __Host-lodestar_session={session}'"
        if marked:
            headers += " " + mark
        out = router.succeed(
            f"curl -s -X {method} {headers} -w '\\n%{{http_code}}' {entrance}{path}"
        )
        body, code = out.rsplit("\n", 1)
        return int(code), body

    def call_json(method, path, session=None, marked=False):
        code, body = call(method, path, session, marked)
        return code, json.loads(body) if body else None

    def login(name):
        return router.succeed(
            f"kanidm-login --server {entrance} --kanidm https://${kanidmDomain} "
            f"--user {name} --password-file /tmp/{name}.password"
        ).strip()

    kanidm.wait_for_unit("kanidm.service")
    router.wait_until_succeeds("curl -sf https://${kanidmDomain}/status")
    kanidm_config = kanidm.succeed(
        "systemctl show -p ExecStart --value kanidm.service | grep -o '/nix/store/[^ ;]*-server.toml'"
    ).split()[0]
    for name, password in passwords.items():
        kanidm.succeed(
            f"runuser -u kanidm -- env KANIDM_RECOVER_ACCOUNT_PASSWORD={password} "
            f"${kanidmPackage}/bin/kanidmd scripting recover-account -c {kanidm_config} "
            f"{name} --from-environment"
        )
        router.succeed(f"echo {password} > /tmp/{name}.password")
    router.wait_until_succeeds(f"curl -sf {entrance}/api/me")

    # The groups claim Kanidm puts in the ID token (logged for the record).
    claims = json.loads(router.succeed(
        "kanidm-login --id-token-claims --kanidm https://${kanidmDomain} --user bob "
        "--password-file /tmp/bob.password --client-secret-file ${clientSecretFile} "
        "--redirect-uri ${publicUrl}/auth/callback"
    ))
    print(f"ID token for bob: preferred_username={claims.get('preferred_username')} groups={claims.get('groups')}")
    assert claims["preferred_username"] == "bob", claims
    assert "admins@${kanidmDomain}" in claims["groups"], claims

    # Anonymous: the reduced Services feed, nothing else.
    code, services = call_json("GET", "/api/public/services")
    assert code == 200 and services, (code, services)
    assert sorted(services[0]) == ["description", "group", "icon", "since", "state", "title"], services[0]
    code, _ = call("GET", "/api/snapshot")
    assert code == 401, code

    # alice (reader): sees the dashboard, cannot act even over Tailscale.
    alice = login("alice")
    code, me = call_json("GET", "/api/me", alice)
    assert code == 200 and me["viewer"] == "alice" and me["role"] == "reader", me
    code, _ = call("GET", "/api/snapshot", alice)
    assert code == 200, code
    code, body = call_json("POST", "/api/hosts/laptop1/apply", alice, marked=True)
    assert code == 403 and body["error"] == "only admins can do this", (code, body)

    # bob (admin): acts only with the router's tailnet mark; the audit names him.
    bob = login("bob")
    code, me = call_json("GET", "/api/me", bob, marked=True)
    assert me["role"] == "admin" and me["can_act"], me
    code, body = call_json("POST", "/api/hosts/laptop1/apply", bob)
    assert code == 403 and body["error"] == "only over Tailscale", (code, body)
    code, body = call_json("POST", "/api/hosts/laptop1/apply", bob, marked=True)
    assert code == 202, (code, body)
    code, events = call_json("GET", "/api/audit?limit=5", bob)
    assert code == 200 and any(e["actor"] == "bob" for e in events), events

    # carol (in neither group): no session.
    out = router.fail(
        f"kanidm-login --server {entrance} --kanidm https://${kanidmDomain} "
        "--user carol --password-file /tmp/carol.password 2>&1"
    )
    print(f"carol: {out.strip()}")
    assert "__Host" not in out and "failed at" in out, out

    # bob logs out: his cookie is anonymous from then on.
    code, _ = call("POST", "/auth/logout", bob)
    assert code == 204, code
    code, me = call_json("GET", "/api/me", bob)
    assert me["role"] == "anonymous" and not me["session"], me

    # Only the router gets in: the manager itself is refused, other hosts are dropped.
    out = manager.succeed(f"curl -s -w '\\n%{{http_code}}' {mark} {entrance}/api/me")
    assert out.rsplit("\n", 1)[1] == "403", out
    status, _ = kanidm.execute("curl -s --connect-timeout 5 http://manager:8081/api/me")
    assert status == 28, f"expected curl exit 28 (dropped), got {status}"
  '';
}
