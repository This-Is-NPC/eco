#!/usr/bin/env bash

eco_wayland_env() {
  local entry candidate socket
  if [[ -z ${XDG_RUNTIME_DIR:-} ]]; then
    XDG_RUNTIME_DIR=/run/user/$UID
  fi

  if [[ -n ${WAYLAND_DISPLAY:-} ]]; then
    socket=$WAYLAND_DISPLAY
    [[ $socket == /* ]] || socket=$XDG_RUNTIME_DIR/$socket
    [[ -S $socket ]] || unset WAYLAND_DISPLAY
  fi

  if [[ -z ${WAYLAND_DISPLAY:-} || -z ${HYPRLAND_INSTANCE_SIGNATURE:-} ]]; then
    while IFS= read -r entry; do
      case $entry in
        WAYLAND_DISPLAY=*)
          if [[ -z ${WAYLAND_DISPLAY:-} ]]; then
            candidate=${entry#WAYLAND_DISPLAY=}
            socket=$candidate
            [[ $socket == /* ]] || socket=$XDG_RUNTIME_DIR/$socket
            [[ -S $socket ]] && WAYLAND_DISPLAY=$candidate
          fi
          ;;
        HYPRLAND_INSTANCE_SIGNATURE=*)
          if [[ -z ${HYPRLAND_INSTANCE_SIGNATURE:-} ]]; then
            HYPRLAND_INSTANCE_SIGNATURE=${entry#HYPRLAND_INSTANCE_SIGNATURE=}
          fi
          ;;
      esac
    done < <(systemctl --user show-environment 2>/dev/null)
  fi

  if [[ -z ${WAYLAND_DISPLAY:-} ]]; then
    candidate=""
    for socket in "$XDG_RUNTIME_DIR"/wayland-*; do
      [[ -S $socket ]] || continue
      if [[ -n $candidate ]]; then
        printf 'eco: multiple Wayland displays; set WAYLAND_DISPLAY explicitly\n' >&2
        return 1
      fi
      candidate=${socket##*/}
    done
    WAYLAND_DISPLAY=$candidate
  fi

  if [[ -z $WAYLAND_DISPLAY ]]; then
    printf 'eco: no Wayland session found; start from an Omarchy desktop\n' >&2
    return 1
  fi

  QT_QPA_PLATFORM=wayland
  export XDG_RUNTIME_DIR WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE QT_QPA_PLATFORM
}
