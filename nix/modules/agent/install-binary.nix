# Activation snippet that installs the agent binary at a fixed path (`binaryPath`), so
# the daemon and the apply wrapper run from a path that does not change with every
# build. The copy only happens when the package's store path changed (stamp file next
# to the binary) and is replaced atomically (`mv`), so a running agent or apply wrapper
# keeps its old inode.
#
# On macOS (`darwin.sign`) the copy is signed before it is moved into place with a
# local self-signed certificate under `darwin.dir`, created once per Mac, so TCC and
# the application firewall see one stable code identity (the designated requirement
# names the identifier and the certificate, not the code hash) across updates. Signing
# uses a throwaway keychain in a root-only temporary directory, never the System or
# login keychain. Every step fails safe: on error it warns and the agent runs from the
# copied, ad-hoc signed binary.
{
  pkgs,
  lib,
  source,
  binaryPath,
  # Darwin only: { sign = bool; dir = signing state dir; certificate = PEM text or
  # null; firewall = bool; }.
  darwin ? null,
}:
let
  bin = {
    cat = "${pkgs.coreutils}/bin/cat";
    chmod = "${pkgs.coreutils}/bin/chmod";
    chown = "${pkgs.coreutils}/bin/chown";
    cp = "${pkgs.coreutils}/bin/cp";
    mkdir = "${pkgs.coreutils}/bin/mkdir";
    mktemp = "${pkgs.coreutils}/bin/mktemp";
    mv = "${pkgs.coreutils}/bin/mv";
    rm = "${pkgs.coreutils}/bin/rm";
    tr = "${pkgs.coreutils}/bin/tr";
    grep = "${pkgs.gnugrep}/bin/grep";
    sed = "${pkgs.gnused}/bin/sed";
    openssl = "${lib.getBin pkgs.openssl}/bin/openssl";
    # macOS system tools.
    security = "/usr/bin/security";
    codesign = "/usr/bin/codesign";
    socketfilterfw = "/usr/libexec/ApplicationFirewall/socketfilterfw";
  };

  identifier = "org.lodestar.agent";
  dir = builtins.dirOf binaryPath;
  stamp = "${dir}/.lodestar-agent.source";
  new = "${dir}/.lodestar-agent.new";

  sign = darwin != null && darwin.sign;
  certificate = if sign then darwin.certificate or null else null;
  certificateFile = pkgs.writeText "lodestar-agent-signing.pem" certificate;
  firewall = darwin != null && darwin.firewall;

  # Darwin: ensure_identity, signed_ok, sign_file.
  signingFunctions = lib.optionalString sign ''
    signing_dir=${lib.escapeShellArg darwin.dir}
    sha1=""

    # Sets sha1 from the configured certificate (and a key placed for it), or creates
    # a key and certificate once. Returns non-zero on failure.
    ensure_identity() {
      ${
        if certificate != null then
          ''
            ${bin.mkdir} -p "$signing_dir" && ${bin.chmod} 0700 "$signing_dir" || return 1
            if [ ! -s "$signing_dir/key.pem" ]; then
              echo "lodestar-agent: no signing key in $signing_dir; place it once with: <key.pem sudo lodestar-agent-signing-key" >&2
              return 1
            fi
            if [ "$(${bin.openssl} x509 -in ${certificateFile} -noout -pubkey)" != \
              "$(${bin.openssl} pkey -in "$signing_dir/key.pem" -pubout 2>/dev/null)" ]; then
              echo "lodestar-agent: $signing_dir/key.pem does not belong to the configured certificate" >&2
              return 1
            fi
            ${bin.cat} ${certificateFile} > "$signing_dir/cert.pem" || return 1
          ''
        else
          ''
            if [ ! -s "$signing_dir/key.pem" ] || [ ! -s "$signing_dir/cert.pem" ]; then
              echo "lodestar-agent: creating the local code-signing certificate in $signing_dir" >&2
              (
                umask 077
                ${bin.mkdir} -p "$signing_dir" &&
                  ${bin.chmod} 0700 "$signing_dir" &&
                  ${bin.rm} -f "$signing_dir/key.pem.new" "$signing_dir/cert.pem.new" &&
                  ${bin.openssl} req -x509 -newkey rsa:2048 -nodes -days 36500 \
                    -subj "/CN=Lodestar Agent Code Signing" \
                    -addext "keyUsage=critical,digitalSignature" \
                    -addext "extendedKeyUsage=critical,codeSigning" \
                    -addext "basicConstraints=critical,CA:false" \
                    -keyout "$signing_dir/key.pem.new" -out "$signing_dir/cert.pem.new" 2>/dev/null &&
                  ${bin.mv} -f "$signing_dir/key.pem.new" "$signing_dir/key.pem" &&
                  ${bin.mv} -f "$signing_dir/cert.pem.new" "$signing_dir/cert.pem"
              ) || return 1
            fi
          ''
      }
      ${bin.chmod} 0700 "$signing_dir" || return 1
      sha1="$(${bin.openssl} x509 -in "$signing_dir/cert.pem" -noout -fingerprint -sha1 |
        ${bin.sed} 's/.*=//; s/://g' | ${bin.tr} -d '[:space:]')" || return 1
      [ -n "$sha1" ] || return 1
      echo "$sha1" > "$signing_dir/cert.sha1"
    }

    requirement() {
      echo "identifier \"${identifier}\" and certificate leaf = H\"$sha1\""
    }

    # True when the binary at $1 carries the stable identity.
    signed_ok() {
      [ -n "$sha1" ] && ${bin.codesign} -v -R="$(requirement)" "$1" >/dev/null 2>&1
    }

    # Signs $1 in place with a throwaway keychain.
    sign_file() {
      local work password keychain status
      work="$(${bin.mktemp} -d "$signing_dir/sign.XXXXXX")" || return 1
      password="$(${bin.openssl} rand -hex 32)" || { ${bin.rm} -rf "$work"; return 1; }
      keychain="$work/sign.keychain-db"
      status=0
      LODESTAR_P12_PASS="$password" ${bin.openssl} pkcs12 -export \
          -inkey "$signing_dir/key.pem" -in "$signing_dir/cert.pem" -name lodestar-agent \
          -passout env:LODESTAR_P12_PASS -out "$work/identity.p12" &&
        ${bin.security} create-keychain -p "$password" "$keychain" &&
        ${bin.security} set-keychain-settings "$keychain" &&
        ${bin.security} unlock-keychain -p "$password" "$keychain" &&
        ${bin.security} import "$work/identity.p12" -k "$keychain" -P "$password" \
          -f pkcs12 -T ${bin.codesign} >/dev/null &&
        ${bin.security} set-key-partition-list -S apple-tool:,apple: -s -k "$password" \
          "$keychain" >/dev/null &&
        ${bin.codesign} --force --keychain "$keychain" -s "$sha1" -i ${identifier} \
          -r="designated => $(requirement)" "$1" ||
        status=1
      ${bin.security} delete-keychain "$keychain" >/dev/null 2>&1 || true
      ${bin.rm} -rf "$work"
      return "$status"
    }
  '';

  firewallFunction = lib.optionalString firewall ''
    # Allows incoming connections for the binary when the application firewall is on.
    firewall_allow() {
      [ -x ${bin.socketfilterfw} ] || return 0
      if ${bin.socketfilterfw} --getglobalstate 2>/dev/null | ${bin.grep} -q "State = [12]"; then
        ${bin.socketfilterfw} --add "$dst" >/dev/null 2>&1 &&
          ${bin.socketfilterfw} --unblockapp "$dst" >/dev/null 2>&1 ||
          echo "lodestar-agent: warning: could not allow $dst in the application firewall" >&2
      fi
    }
  '';
in
''
  (
    src=${lib.escapeShellArg source}
    dst=${lib.escapeShellArg binaryPath}
    dir=${lib.escapeShellArg dir}
    stamp=${lib.escapeShellArg stamp}
    new=${lib.escapeShellArg new}
    ${signingFunctions}
    ${firewallFunction}
    ${bin.mkdir} -p "$dir" && ${bin.chown} 0:0 "$dir" && ${bin.chmod} 0755 "$dir" ||
      echo "lodestar-agent: warning: could not prepare $dir" >&2

    ${lib.optionalString sign ''
      ensure_identity || {
        sha1=""
        echo "lodestar-agent: warning: no code-signing certificate in $signing_dir; the agent stays ad-hoc signed" >&2
      }
    ''}
    install=0
    [ "$(${bin.cat} "$stamp" 2>/dev/null || true)" = "$src" ] || install=1
    [ -x "$dst" ] || install=1
    ${lib.optionalString sign ''
      # Also after a reset of the certificate, or when an earlier signing failed.
      if [ -n "$sha1" ] && ! signed_ok "$dst"; then install=1; fi
    ''}
    if [ "$install" = 1 ]; then
      ${bin.rm} -f "$new"
      if ${bin.cp} "$src" "$new" && ${bin.chown} 0:0 "$new" && ${bin.chmod} 0755 "$new"; then
        ${lib.optionalString sign ''
          if [ -n "$sha1" ]; then
            if sign_file "$new" && signed_ok "$new"; then
              echo "lodestar-agent: signed $dst ($(requirement))" >&2
            else
              echo "lodestar-agent: warning: signing $dst failed; it stays ad-hoc signed (macOS will ask for permissions again)" >&2
              ${bin.rm} -f "$new"
              ${bin.cp} "$src" "$new" && ${bin.chown} 0:0 "$new" && ${bin.chmod} 0755 "$new"
            fi
          fi
        ''}
        if ${bin.mv} -f "$new" "$dst"; then
          echo "$src" > "$stamp"
        else
          echo "lodestar-agent: warning: could not install $dst" >&2
        fi
      else
        ${bin.rm} -f "$new"
        echo "lodestar-agent: warning: could not copy $src to $new" >&2
      fi
    fi
    ${lib.optionalString firewall "firewall_allow"}
  ) || true
''
