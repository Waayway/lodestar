# The cache host's scripts (used by nix/modules/cache.nix):
#
# - `cache-publish <commit> <store-path>…` (channel index JSON on stdin): writes the paths
#   of the given closures that were built on this machine (`ultimate`) into the file
#   binary cache at `cacheDir` (zstd NARs, narinfos signed with `signingKeyFile`; their
#   references stay on cache.nixos.org), merges them into
#   `.roots/<commit>`, stores the index as `.roots/channels.json` and runs `cache-prune`
#   keeping `<commit>` too (a publish never deletes what it just wrote, also when the
#   commit is no channel's current or previous one).
#   Paths substituted from this cache (no longer `ultimate`, but signed with this cache's
#   key) count as built here, so they stay in the commit's roots.
# - `cache-prune [cache-dir [keep-commit]]`: deletes every narinfo (and its NAR) whose
#   store path is not listed in the `.roots` file of a channel's current or previous
#   commit (or of `keep-commit`), then the `.roots` files of commits not kept. Deletes
#   nothing without a readable index.
#
# Both serialise on `<cacheDir>/.lock`. `LODESTAR_CACHE_DIR` and
# `LODESTAR_CACHE_SIGNING_KEY` override the built-in paths of `cache-publish` (tests).
{
  lib,
  writeShellApplication,
  symlinkJoin,
  nix,
  jq,
  coreutils,
  findutils,
  flock,
  zstd,
  cacheDir ? "/mnt/nix-cache",
  signingKeyFile ? null,
}:
let
  # Shared by both scripts: the formats the Lodestar server validates too.
  patterns = ''
    nix32='[0123456789abcdfghijklmnpqrsvwxyz]'
    commitPattern='^[0-9a-f]{40}$'
    storePathPattern="^/nix/store/($nix32{32})-[A-Za-z0-9+_=-][A-Za-z0-9+._=-]*\$"
    # A channel index: { "<channel>": { "current": "<commit>", "previous": "<commit>"|null } }.
    # Prints the commits it names; fails on any other shape.
    indexCommits='
      if type != "object" then error("not an object") else . end
      | .[]
      | if type == "object" and (.current | type) == "string"
          and ((.previous | type) == "string" or .previous == null)
        then .current, (.previous // empty)
        else error("not a channel entry") end'
  '';

  cache-prune = writeShellApplication {
    name = "cache-prune";
    runtimeInputs = [
      coreutils
      findutils
      jq
      flock
    ];
    text = ''
      ${patterns}
      hashPattern="^$nix32{32}\$"
      narUrlPattern='^nar/[A-Za-z0-9]+\.nar(\.[A-Za-z0-9]+)?$'
      cacheDir=''${1:-${cacheDir}}
      extraCommit=''${2:-}
      log() { echo "cache-prune: $*" >&2; }
      # The `URL:` a narinfo names, if it is a file directly in nar/ (else nothing).
      narUrl() {
        local line
        while IFS= read -r line || [ -n "$line" ]; do
          if [[ $line == "URL: "* ]]; then
            [[ ''${line#URL: } =~ $narUrlPattern ]] && echo "''${line#URL: }"
            return 0
          fi
        done <"$1"
      }
      umask 022

      if [ ! -d "$cacheDir" ] || [ ! -w "$cacheDir" ]; then
        log "$cacheDir is not a writable directory (is the cache mounted?)"
        exit 1
      fi
      # cache-publish holds the lock already and runs this with LODESTAR_CACHE_LOCKED=1.
      if [ -z "''${LODESTAR_CACHE_LOCKED:-}" ]; then
        exec 9>>"$cacheDir/.lock"
        flock 9
      fi

      roots=$cacheDir/.roots
      index=$roots/channels.json
      if [ ! -f "$index" ] || [ -L "$index" ]; then
        log "no $index: deleting nothing"
        exit 0
      fi
      if ! commits=$(jq -r "$indexCommits" "$index" 2>/dev/null); then
        log "$index is not a channel index: deleting nothing"
        exit 0
      fi

      if [ -z "$commits" ]; then
        log "$index keeps no commit: deleting nothing"
        exit 0
      fi

      if [ -n "$extraCommit" ]; then
        commits+=$'\n'$extraCommit
      fi

      # The keep set: the store-path hashes listed for every kept commit.
      declare -A keepCommits=() keep=()
      while IFS= read -r commit; do
        if ! [[ $commit =~ $commitPattern ]]; then
          log "invalid kept commit $(printf %q "$commit"): deleting nothing"
          exit 0
        fi
        if [ ! -f "$roots/$commit" ] || [ -L "$roots/$commit" ]; then
          log "no roots for kept commit $commit: deleting nothing"
          exit 0
        fi
        keepCommits[$commit]=1
        while IFS= read -r path || [ -n "$path" ]; do
          if [[ $path =~ $storePathPattern ]]; then
            keep[''${BASH_REMATCH[1]}]=1
          fi
        done <"$roots/$commit"
      done <<<"$commits"

      # Narinfos (regular files directly in the cache, named by a store-path hash). NARs
      # of kept narinfos are never deleted, even when an unkept one names the same file.
      declare -A keptNars=()
      drop=()
      while IFS= read -r -d "" info; do
        name=''${info##*/}
        hash=''${name%.narinfo}
        [[ $hash =~ $hashPattern ]] || continue
        if [ -n "''${keep[$hash]:-}" ]; then
          url=$(narUrl "$info")
          [ -n "$url" ] && keptNars[$url]=1
        else
          drop+=("$info")
        fi
      done < <(find "$cacheDir/" -maxdepth 1 -type f -name '*.narinfo' -print0)

      narinfos=0 nars=0
      for info in "''${drop[@]}"; do
        url=$(narUrl "$info")
        rm -f -- "$info"
        narinfos=$((narinfos + 1))
        # Only plain files inside nar/: never a symlink, never `..`.
        nar=$cacheDir/$url
        if [ -n "$url" ] && [ -z "''${keptNars[$url]:-}" ] && [ -f "$nar" ] && [ ! -L "$nar" ]; then
          rm -f -- "$nar"
          nars=$((nars + 1))
        fi
      done

      rootFiles=0
      while IFS= read -r -d "" file; do
        commit=''${file##*/}
        if [[ $commit =~ $commitPattern ]] && [ -z "''${keepCommits[$commit]:-}" ]; then
          rm -f -- "$file"
          rootFiles=$((rootFiles + 1))
        fi
      done < <(find "$roots/" -maxdepth 1 -type f -print0)

      log "kept ''${#keep[@]} paths of ''${#keepCommits[@]} commits; deleted $narinfos narinfos, $nars NARs, $rootFiles roots files"
    '';
  };

  cache-publish = writeShellApplication {
    name = "cache-publish";
    runtimeInputs = [
      coreutils
      jq
      nix
      flock
      zstd
    ];
    text = ''
      ${patterns}
      cacheDir=''${LODESTAR_CACHE_DIR:-${cacheDir}}
      signingKeyFile=''${LODESTAR_CACHE_SIGNING_KEY:-${toString signingKeyFile}}
      die() { echo "cache-publish: $*" >&2; exit 1; }
      umask 022

      [ $# -ge 2 ] || die "usage: cache-publish <commit> <store-path>... (channel index JSON on stdin)"
      commit=$1
      shift
      [[ $commit =~ $commitPattern ]] || die "invalid commit $(printf %q "$commit")"
      for path in "$@"; do
        [[ $path =~ $storePathPattern ]] || die "not a store path: $(printf %q "$path")"
      done

      workDir=$(mktemp -d)
      trap 'rm -rf "$workDir"' EXIT
      cat >"$workDir/channels.json"
      commits=$(jq -r "$indexCommits" "$workDir/channels.json" 2>/dev/null) \
        || die "stdin is not a channel index ({\"<channel>\": {\"current\": …, \"previous\": …}})"
      while IFS= read -r indexCommit; do
        [ -z "$indexCommit" ] || [[ $indexCommit =~ $commitPattern ]] \
          || die "the channel index names an invalid commit $(printf %q "$indexCommit")"
      done <<<"$commits"

      # Plain paths only (they end up in messages and shell words).
      safePath='^/[A-Za-z0-9._/-]+$'
      [[ $cacheDir =~ $safePath ]] || die "unsupported cache directory $(printf %q "$cacheDir")"
      [ -d "$cacheDir" ] && [ -w "$cacheDir" ] \
        || die "$cacheDir is not a writable directory (is the cache mounted?)"
      [ -n "$signingKeyFile" ] || die "no signing key configured"
      [[ $signingKeyFile =~ $safePath ]] || die "unsupported signing key path $(printf %q "$signingKeyFile")"
      [ -r "$signingKeyFile" ] || die "signing key $signingKeyFile is not readable"

      roots=$cacheDir/.roots
      mkdir -p "$roots"
      exec 9>>"$cacheDir/.lock"
      flock 9

      # Writes stdin to $1 via a temporary file in the same directory and a rename.
      writeAtomically() {
        local tmp
        tmp=$(mktemp "$(dirname "$1")/.tmp.XXXXXX")
        cat >"$tmp"
        chmod 0644 "$tmp"
        mv -f "$tmp" "$1"
      }

      if [ ! -e "$cacheDir/nix-cache-info" ]; then
        printf 'StoreDir: /nix/store\nWantMassQuery: 1\nPriority: 50\n' \
          | writeAtomically "$cacheDir/nix-cache-info"
      fi

      # The closure, reduced to the paths built here (`ultimate`) or substituted from this
      # cache (signed with its key: after a store GC the builder gets unchanged paths back
      # from here, and they must stay in the commit's roots); the rest is signed by
      # cache.nixos.org and comes from there. Older nix prints an array, newer an object.
      keyName=$(cut -d: -f1 "$signingKeyFile")
      [ -n "$keyName" ] || die "signing key $signingKeyFile has no key name"
      jsonFormat=()
      if [[ $(nix path-info --help 2>/dev/null || true) == *--json-format* ]]; then
        jsonFormat=(--json-format 1)
      fi
      nix path-info --recursive --json "''${jsonFormat[@]}" -- "$@" >"$workDir/closure.json"
      published=()
      while IFS= read -r path; do
        [[ $path =~ $storePathPattern ]] || die "nix path-info returned $(printf %q "$path")"
        published+=("$path")
      done < <(jq -r --arg key "$keyName" '
        (if type == "array" then map({ key: .path, value: . }) | from_entries else . end)
        | to_entries[]
        | select(.value != null and (.value.ultimate == true
            or ((.value.signatures // []) | any(startswith($key + ":")))))
        | .key' "$workDir/closure.json" | sort)

      # Binary-cache entries for the paths not published yet, written here rather than
      # with `nix copy`: a file cache refuses a path whose references it lacks, and the
      # references live on cache.nixos.org (clients substitute each path independently).
      fresh=()
      for path in "''${published[@]}"; do
        base=''${path#/nix/store/}
        [ -e "$cacheDir/''${base%%-*}.narinfo" ] || fresh+=("$path")
      done
      if [ ''${#fresh[@]} -gt 0 ]; then
        mkdir -p "$cacheDir/nar"
        nix store sign --key-file "$signingKeyFile" -- "''${fresh[@]}"
        nix path-info --json "''${jsonFormat[@]}" -- "''${fresh[@]}" >"$workDir/fresh.json"
        for path in "''${fresh[@]}"; do
          jq --arg path "$path" --arg key "$keyName" '
            (if type == "array" then map({ key: .path, value: . }) | from_entries else . end)[$path]
            | {
                narHash,
                narSize,
                references: (.references | map(sub("^/nix/store/"; "")) | join(" ")),
                deriver: ((.deriver // "") | sub("^/nix/store/"; "")),
                sig: ((.signatures // []) | map(select(startswith($key + ":"))) | first // "")
              }' "$workDir/fresh.json" >"$workDir/entry.json"
          field() { jq -r ".$1" "$workDir/entry.json"; }
          narHash=$(field narHash)
          case $narHash in
            sha256:*) ;;
            sha256-*) narHash=sha256:$(nix hash convert --hash-algo sha256 --to nix32 "$narHash") ;;
            *) die "unexpected narHash $(printf %q "$narHash") for $path" ;;
          esac
          sig=$(field sig)
          [ -n "$sig" ] || die "no $keyName signature on $path after signing"

          tmp=$(mktemp "$cacheDir/nar/.tmp.XXXXXX")
          nix-store --dump "$path" | zstd -q -T0 -c >"$tmp"
          fileHash=$(nix-hash --type sha256 --flat --base32 "$tmp")
          fileSize=$(stat -c %s "$tmp")
          chmod 0644 "$tmp"
          mv -f "$tmp" "$cacheDir/nar/$fileHash.nar.zst"

          base=''${path#/nix/store/}
          deriver=$(field deriver)
          {
            echo "StorePath: $path"
            echo "URL: nar/$fileHash.nar.zst"
            echo "Compression: zstd"
            echo "FileHash: sha256:$fileHash"
            echo "FileSize: $fileSize"
            echo "NarHash: $narHash"
            echo "NarSize: $(field narSize)"
            echo "References: $(field references)"
            if [ -n "$deriver" ]; then echo "Deriver: $deriver"; fi
            echo "Sig: $sig"
          } | writeAtomically "$cacheDir/''${base%%-*}.narinfo"
        done
      fi

      # A commit can be published in parts (a retry builds only the hosts that failed):
      # its roots are the union of every publish.
      {
        if [ -f "$roots/$commit" ] && [ ! -L "$roots/$commit" ]; then cat "$roots/$commit"; fi
        if [ ''${#published[@]} -gt 0 ]; then printf '%s\n' "''${published[@]}"; fi
      } | sort -u | writeAtomically "$roots/$commit"
      writeAtomically "$roots/channels.json" <"$workDir/channels.json"
      echo "cache-publish: $commit: ''${#published[@]} of $(jq length "$workDir/closure.json") paths built here" >&2

      LODESTAR_CACHE_LOCKED=1 ${lib.getExe cache-prune} "$cacheDir" "$commit"
    '';
  };
in
symlinkJoin {
  name = "lodestar-cache-scripts";
  paths = [
    cache-publish
    cache-prune
  ];
}
