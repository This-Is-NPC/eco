#!/usr/bin/env bash
# stage_files <root> <prefix> puts what eco ships beside its binary under
# <root><prefix>: the window program (built by scripts/window-build) in
# lib/eco, which is eco's alone, the overlay it runs, the Hyprland rules, the
# launcher, the icon, the shell completions (written to target/completions by
# scripts/completions) and the licence. Run from the repository root.
stage_files() {
  local share="$1$2/share"
  install -Dm755 target/window/eco-window "$1$2/lib/eco/eco-window"
  rm -rf "$share/eco/overlay"
  install -d "$share/eco"
  cp -r overlay "$share/eco/overlay"
  install -Dm644 packaging/hypr/eco.lua "$share/eco/hypr/eco.lua"
  install -Dm644 packaging/eco.desktop "$share/applications/eco.desktop"
  install -Dm644 packaging/eco.svg "$share/icons/hicolor/scalable/apps/eco.svg"
  install -Dm644 target/completions/eco.bash "$share/bash-completion/completions/eco"
  install -Dm644 target/completions/_eco "$share/zsh/site-functions/_eco"
  install -Dm644 target/completions/eco.fish "$share/fish/vendor_completions.d/eco.fish"
  install -Dm644 LICENSE "$share/licenses/eco/LICENSE"
}

# service_unit <binary> prints the user service that runs <binary>.
service_unit() {
  sed "s|@ECO_BINARY@|$1|" packaging/eco.service
}
