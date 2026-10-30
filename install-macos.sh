#!/usr/bin/env bash
# install-macos.sh - install eco on macOS from a GitHub release, for you alone.
#
#   curl -fsSL https://raw.githubusercontent.com/This-Is-NPC/eco/master/install-macos.sh | bash
#
# It downloads the macOS archive a release carries, checks it against the
# release's SHA256SUMS, puts its eco folder in ~/Applications/eco and runs
# `eco setup`. A daemon that was running is stopped first and started again.
#
# Environment:
#   VERSION        release to install, e.g. 0.1.0 or v0.1.0 (default: the latest)
#   REPO           GitHub repository (default: This-Is-NPC/eco)
#   BASE_URL       where the release files are, instead of GitHub's download URL
#   DRY_RUN=1      download and verify, then say what would be done
#   ECO_UNINSTALL  keep: take eco off, keep sessions, config and models;
#                  everything: take eco off and delete those too
set -euo pipefail

REPO="${REPO:-This-Is-NPC/eco}"
VERSION="${VERSION:-}"
BASE_URL="${BASE_URL:-}"
DRY_RUN="${DRY_RUN:-0}"
ECO_UNINSTALL="${ECO_UNINSTALL:-}"

eco="$HOME/Applications/eco"
exe="$eco/bin/eco"
data="$HOME/Library/Application Support/eco"
agent="$HOME/Library/LaunchAgents/io.github.this-is-npc.eco.plist"

say() { printf 'eco: %s\n' "$*" >&2; }
die() { say "$*"; exit 1; }

[[ "$(uname -s)" == Darwin ]] || die "this installs eco on macOS; on Linux use install.sh."

# Stop the installed daemon. Prints "window" when its window was open,
# "daemon" when only the daemon ran, nothing when none ran.
stop_eco() {
  [[ -x "$exe" ]] || return 0
  local status=0
  "$exe" status >/dev/null 2>&1 || status=$?
  if [[ $status == 0 ]]; then
    if "$exe" status --window-open >/dev/null 2>&1; then echo window; else echo daemon; fi
    "$exe" stop >&2 || die "could not stop the running eco; nothing was changed."
  elif [[ $status != 3 ]]; then
    die "cannot tell whether the eco daemon is running; nothing was changed."
  fi
}

uninstall_eco() {
  local everything="$1" path
  local paths=("$eco" "$agent")
  [[ "$everything" == 1 ]] && paths+=("$data")
  if [[ "$DRY_RUN" == 1 ]]; then
    for path in "${paths[@]}"; do [[ -e "$path" ]] && say "would remove $path"; done
    return 0
  fi
  stop_eco >/dev/null
  launchctl bootout "gui/$(id -u)/io.github.this-is-npc.eco" 2>/dev/null || true
  for path in "${paths[@]}"; do
    if [[ -e "$path" ]]; then rm -rf "$path"; say "removed $path"; fi
  done
  [[ "$everything" == 1 || ! -e "$data" ]] || say "kept $data"
}

install_eco() {
  [[ "$(uname -m)" == arm64 ]] ||
    die "there is only an Apple Silicon build, and this Mac is $(uname -m)."
  if [[ -z "$BASE_URL" ]]; then
    local tag
    if [[ -z "$VERSION" ]]; then
      tag="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" |
        sed -nE '/"tag_name"/{s/.*"tag_name" *: *"([^"]+)".*/\1/p;q;}')" ||
        die "found no published release of ${REPO}."
      [[ -n "$tag" ]] || die "${REPO} has no release yet."
    else
      tag="v${VERSION#v}"
    fi
    BASE_URL="https://github.com/${REPO}/releases/download/${tag}"
  fi
  BASE_URL="${BASE_URL%/}"

  mkdir -p "$(dirname "$eco")"
  # Beside the install, so the folder moves in by a rename.
  tmp="$(mktemp -d "$(dirname "$eco")/eco-install.XXXXXX")"
  trap 'rm -rf "$tmp"' EXIT

  curl -fsSL "${BASE_URL}/SHA256SUMS" -o "$tmp/SHA256SUMS" ||
    die "could not download ${BASE_URL}/SHA256SUMS."
  local archive
  archive="$(awk '$2 ~ /^\*?eco-[0-9][^ ]*-macos-aarch64\.tar\.gz$/ { sub(/^\*/, "", $2); print $2; exit }' "$tmp/SHA256SUMS")"
  [[ -n "$archive" ]] || die "the release's SHA256SUMS names no macOS archive."

  say "downloading ${archive}"
  curl -fsSL "${BASE_URL}/${archive}" -o "$tmp/${archive}" ||
    die "could not download ${BASE_URL}/${archive}."
  (cd "$tmp" && grep -F " ${archive}" SHA256SUMS | shasum -a 256 -c --quiet -) ||
    die "${archive} does not match the release's SHA256SUMS; nothing was installed."
  if [[ "$DRY_RUN" == 1 ]]; then
    say "${archive} matches SHA256SUMS; would install it in $eco and run eco setup"
    return 0
  fi

  mkdir "$tmp/unpacked"
  tar -x -z -f "$tmp/${archive}" -C "$tmp/unpacked"
  [[ -x "$tmp/unpacked/eco/bin/eco" ]] ||
    die "${archive} holds no eco/bin/eco; nothing was installed."
  local was
  was="$(stop_eco)"
  rm -rf "$eco"
  mv "$tmp/unpacked/eco" "$eco"
  say "installed eco in $eco"

  "$exe" setup || die "eco setup failed; run it again to see why."
  if [[ -n "$was" ]]; then
    "$exe" start --headless
    [[ "$was" == window ]] && "$exe" start
  fi
  case ":$PATH:" in
    *":$eco/bin:"*) ;;
    *) say "add $eco/bin to your PATH, e.g. in ~/.zprofile: export PATH=\"\$HOME/Applications/eco/bin:\$PATH\"" ;;
  esac
  say "installed. Next: eco start"
  say "remove it with eco uninstall (keeps your data; --everything deletes it too)"
}

case "$ECO_UNINSTALL" in
  "") install_eco ;;
  keep) uninstall_eco 0 ;;
  everything) uninstall_eco 1 ;;
  *) die "ECO_UNINSTALL is keep or everything, not ${ECO_UNINSTALL}." ;;
esac
