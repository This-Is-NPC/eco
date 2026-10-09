#!/usr/bin/env bash
# stage_files <root> <prefix> puts what eco ships beside its binary under
# <root><prefix>: the overlay, the Hyprland rules pointing at <prefix>'s copy of
# it, the launcher, the icon and the licence. Run from the repository root.
stage_files() {
  local prefix="$2" share="$1$2/share"
  rm -rf "$share/eco/overlay"
  install -d "$share/eco/hypr"
  cp -r overlay "$share/eco/overlay"
  sed "s|^local overlay = .*|local overlay = \"$prefix/share/eco/overlay\"|" \
    packaging/hypr/eco.lua > "$share/eco/hypr/eco.lua"
  install -Dm644 packaging/eco.desktop "$share/applications/eco.desktop"
  install -Dm644 packaging/eco.svg "$share/icons/hicolor/scalable/apps/eco.svg"
  install -Dm644 LICENSE "$share/licenses/eco/LICENSE"
}

# service_unit <binary> prints the user service that runs <binary>.
service_unit() {
  sed "s|@ECO_BINARY@|$1|" packaging/eco.service
}
