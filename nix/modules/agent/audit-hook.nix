# Activation hook: a shell snippet that appends one JSON line per system activation
# (including manual `nixos-rebuild`/`darwin-rebuild` runs and boots) to the audit file
# that lodestar-agent serves at GET /activations. Used by the agent modules.
{ pkgs }:
let
  stateDir = "/var/lib/lodestar-agent";
  bin.jq = "${pkgs.jq}/bin/jq";

  hook = ''
    (
      export PATH=${pkgs.coreutils}/bin:$PATH
      state_dir=${stateDir}
      mkdir -p "$state_dir"

      store_path="$systemConfig"

      revision=""
      if [ -x "$systemConfig/sw/bin/nixos-version" ]; then
        revision="$("$systemConfig/sw/bin/nixos-version" --configuration-revision 2>/dev/null || true)"
      fi
      if [ -z "$revision" ] && [ -r "$systemConfig/darwin-version.json" ]; then
        revision="$(${bin.jq} -r '.configurationRevision // empty' "$systemConfig/darwin-version.json" 2>/dev/null || true)"
      fi
      [ -n "$revision" ] || revision=unknown

      generation=""
      link="$(readlink /nix/var/nix/profiles/system 2>/dev/null || true)"
      case "$link" in
        system-*-link)
          generation="''${link#system-}"
          generation="''${generation%-link}"
          ;;
      esac

      sudo_user=""
      if [ -f "$state_dir/apply-in-progress.json" ]; then
        apply_id="$(${bin.jq} -r '.apply_id // empty' "$state_dir/apply-in-progress.json" 2>/dev/null || true)"
        trigger="apply:$apply_id"
      elif [ ! -e /run/current-system ]; then
        trigger=boot
      else
        trigger=manual
        sudo_user="''${SUDO_USER:-}"
      fi

      ${bin.jq} -nc \
        --argjson time "$(date +%s)" \
        --arg store_path "$store_path" \
        --arg revision "$revision" \
        --arg generation "$generation" \
        --arg trigger "$trigger" \
        --arg sudo_user "$sudo_user" \
        '{
          time: $time,
          store_path: $store_path,
          revision: $revision,
          generation: (if $generation == "" then null else ($generation | tonumber) end),
          trigger: $trigger,
          sudo_user: (if $sudo_user == "" then null else $sudo_user end)
        }' >> "$state_dir/activations.jsonl"
    ) || true
  '';
in
hook
