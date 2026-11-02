# Fixture test of the cache host's scripts (nix/packages/cache-scripts.nix), run on any
# system: `cache-prune` against a file binary cache with narinfos A–E (plus shared,
# escaping and symlinked NAR URLs), and `cache-publish` refusing bad arguments or a bad
# channels index without touching the cache directory.
{ pkgs }:
let
  scripts = pkgs.callPackage ../packages/cache-scripts.nix { };
in
pkgs.runCommand "lodestar-cache-prune"
  {
    nativeBuildInputs = [
      scripts
      pkgs.diffutils
    ];
  }
  ''
    set -euo pipefail
    fail() { echo "FAIL: $*" >&2; exit 1; }

    c0=0000000000000000000000000000000000000000
    c1=1111111111111111111111111111111111111111
    c2=2222222222222222222222222222222222222222
    c3=3333333333333333333333333333333333333333
    hash() { printf '%032d' 0 | tr 0 "$1"; }
    A=$(hash 1) B=$(hash 2) C=$(hash 3) D=$(hash 4) E=$(hash 5) G=$(hash 6) H=$(hash 7) I=$(hash 8)

    # narinfo <hash> <name> <url>
    narinfo() {
      printf 'StorePath: /nix/store/%s-%s\nURL: %s\nCompression: zstd\n' "$1" "$2" "$3" \
        > "$dir/$1.narinfo"
    }

    # A fresh cache: A,B published by c1, C by c2, D by c0 (no channel keeps c0), E by
    # nothing. G (unkept) shares A's NAR; H's URL tries to leave nar/; I's NAR is a
    # symlink to a file outside the cache.
    fixture() {
      dir=$(mktemp -d)/cache
      mkdir -p "$dir/nar" "$dir/.roots"
      echo outside > "$(dirname "$dir")/outside.nar.zst"
      echo outside > "$(dirname "$dir")/target"
      for x in A B C D E I; do
        narinfo "''${!x}" "pkg-$x" "nar/nar$x.nar.zst"
        echo "nar $x" > "$dir/nar/nar$x.nar.zst"
      done
      narinfo "$G" pkg-G nar/narA.nar.zst
      narinfo "$H" pkg-H nar/../../outside.nar.zst
      rm "$dir/nar/narI.nar.zst"
      ln -s "$(dirname "$dir")/target" "$dir/nar/narI.nar.zst"
      printf '/nix/store/%s-pkg-A\n/nix/store/%s-pkg-B\n' "$A" "$B" > "$dir/.roots/$c1"
      printf '/nix/store/%s-pkg-C\n' "$C" > "$dir/.roots/$c2"
      printf '/nix/store/%s-pkg-D\n' "$D" > "$dir/.roots/$c0"
      echo 'StoreDir: /nix/store' > "$dir/nix-cache-info"
      : > "$dir/.lock" # left behind by every publish and prune
    }
    channels() {
      printf '{"stable":{"current":"%s","previous":null},"unstable":{"current":"%s","previous":"%s"}}' \
        "$c2" "$c1" "$c2" > "$dir/.roots/channels.json"
    }
    present() { [ -e "$dir/$1" ] || fail "$2: $1 was deleted"; }
    absent() { [ ! -e "$dir/$1" ] && [ ! -L "$dir/$1" ] || fail "$2: $1 was kept"; }
    unchanged() {
      diff -r --no-dereference "$snapshot" "$dir" || fail "$1: the cache directory changed"
    }
    snap() { snapshot=$(mktemp -d)/snap; cp -a "$dir" "$snapshot"; }

    # --- prune with a channel index keeping c1 and c2 ---------------------------------
    fixture
    channels
    cache-prune "$dir"
    for x in A B C; do
      present "''${!x}.narinfo" "kept $x"
      present "nar/nar$x.nar.zst" "kept $x"
    done
    for x in D E G H I; do absent "''${!x}.narinfo" "unkept $x"; done
    absent nar/narD.nar.zst "unkept D"
    absent nar/narE.nar.zst "unkept E"
    present nar/narA.nar.zst "NAR shared by kept A and unkept G"
    [ -e "$(dirname "$dir")/outside.nar.zst" ] || fail "a URL outside nar/ was followed"
    [ -e "$(dirname "$dir")/target" ] || fail "a symlinked NAR's target was deleted"
    absent ".roots/$c0" "roots of c0"
    present ".roots/$c1" "roots of c1"
    present ".roots/$c2" "roots of c2"
    present .roots/channels.json "channels.json"
    present nix-cache-info "nix-cache-info"
    echo "ok: prune keeps the current and previous commits' paths only"

    # --- an extra keep commit (the one cache-publish just published) is kept too --------
    fixture
    channels
    printf '/nix/store/%s-pkg-E\n' "$E" > "$dir/.roots/$c3"
    cache-prune "$dir" "$c3"
    present "$E.narinfo" "kept by the extra commit"
    present nar/narE.nar.zst "kept by the extra commit"
    present ".roots/$c3" "roots of the extra commit"
    present "$A.narinfo" "kept by the index"
    absent "$D.narinfo" "unkept D"
    absent ".roots/$c0" "roots of c0"
    echo "ok: prune keeps the extra commit's paths"

    # An invalid extra commit, or one without roots, deletes nothing.
    for extra in ../x "$c3"; do
      fixture
      channels
      snap
      cache-prune "$dir" "$extra" || fail "prune with extra commit $extra must exit 0"
      unchanged "prune with extra commit $extra"
    done
    echo "ok: prune with a bad or rootless extra commit deletes nothing"

    # --- prune without, or with an unreadable, channels.json deletes nothing ------------
    for index in missing garbage array badcommit empty; do
      fixture
      case $index in
        missing) ;;
        garbage) echo '{not json' > "$dir/.roots/channels.json" ;;
        array) echo '[]' > "$dir/.roots/channels.json" ;;
        badcommit) echo '{"stable":{"current":"../x","previous":null}}' > "$dir/.roots/channels.json" ;;
        empty) echo '{}' > "$dir/.roots/channels.json" ;;
      esac
      snap
      cache-prune "$dir" || fail "prune with a $index channels.json must exit 0"
      unchanged "prune with a $index channels.json"
      echo "ok: prune with a $index channels.json deletes nothing"
    done

    # --- publish refuses bad arguments and a bad index before touching the cache -------
    fixture
    channels
    snap
    good=/nix/store/$A-pkg-A
    index=$(cat "$dir/.roots/channels.json")
    refuses() {
      local what=$1 stdin=$2
      shift 2
      if LODESTAR_CACHE_DIR=$dir cache-publish "$@" <<< "$stdin" 2> err; then
        fail "publish accepted $what"
      fi
      grep -q . err || fail "publish refused $what without a message"
      unchanged "publish with $what"
      echo "ok: publish refuses $what ($(head -n1 err))"
    }
    refuses "no arguments" "$index"
    refuses "no paths" "$index" "$c3"
    refuses "a short commit" "$index" abc123 "$good"
    refuses "an upper-case commit" "$index" "$(echo "$c3" | tr 3 A)" "$good"
    refuses "an option as commit" "$index" --help "$good"
    refuses "a path outside the store" "$index" "$c3" /etc/passwd
    refuses "a short hash" "$index" "$c3" /nix/store/abc-pkg
    refuses "a dot name" "$index" "$c3" "/nix/store/$A-.hidden"
    refuses "a shell word" "$index" "$c3" "/nix/store/$A-x;rm"
    refuses "a sub path" "$index" "$c3" "/nix/store/$A-x/bin"
    refuses "a non-base32 hash" "$index" "$c3" "/nix/store/$(hash e)-x"
    refuses "a bad path after a good one" "$index" "$c3" "$good" ../x
    refuses "invalid JSON on stdin" '{nope' "$c3" "$good"
    refuses "a JSON array on stdin" '[]' "$c3" "$good"
    refuses "a bad commit in the index" '{"stable":{"current":"x","previous":null}}' "$c3" "$good"
    refuses "an index without current" '{"stable":{"previous":null}}' "$c3" "$good"
    if LODESTAR_CACHE_DIR=$dir/missing cache-publish "$c3" "$good" <<< "$index" 2> err; then
      fail "publish accepted a missing cache directory"
    fi
    grep -q "not a writable directory" err || fail "missing cache dir: unclear error: $(cat err)"
    echo "ok: publish refuses a missing cache directory"

    # --- publish of a path the cache has already, absent from the local store ----------
    # (the builder collected it): nothing touches the store, the cache keeps its entry,
    # and the path with the closure the cache has of it (B; not the cache.nixos.org path
    # glibc) is listed in the commit's roots.
    fixture
    channels
    key=$(mktemp -d)/key
    echo 'test-1:AAAA' > "$key"
    glibc=$(hash 9)
    printf 'StorePath: /nix/store/%s-pkg-A\nURL: nar/narA.nar.zst\nCompression: zstd\nReferences: %s-pkg-A %s-pkg-B %s-glibc\n' \
      "$A" "$A" "$B" "$glibc" > "$dir/$A.narinfo"
    [ ! -e "/nix/store/$A-pkg-A" ] || fail "the fixture path is in the store"
    cp "$dir/$A.narinfo" narinfo.before
    cp "$dir/nar/narA.nar.zst" nar.before
    LODESTAR_CACHE_DIR=$dir LODESTAR_CACHE_SIGNING_KEY=$key \
      cache-publish "$c3" "/nix/store/$A-pkg-A" <<< "$index" 2> err \
      || fail "publish of a path the cache has: $(cat err)"
    cmp narinfo.before "$dir/$A.narinfo" || fail "the cached narinfo changed"
    cmp nar.before "$dir/nar/narA.nar.zst" || fail "the cached NAR changed"
    printf '/nix/store/%s-pkg-A\n/nix/store/%s-pkg-B\n' "$A" "$B" | sort > roots.expected
    diff roots.expected "$dir/.roots/$c3" || fail "roots of a path the cache has"
    present "$B.narinfo" "B, in the closure of the published A"
    echo "ok: publish of a path the cache has lists it without touching the store"

    touch $out
  ''
