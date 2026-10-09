#!/usr/bin/env bash
# refresh_desktop_entries rebuilds the launcher and icon caches under a
# prefix, so the menu shows or drops eco without a new login. Both tools are
# optional and a cache that will not rebuild fails nothing.
refresh_desktop_entries() {
  local apps="$1/share/applications" icons="$1/share/icons/hicolor"
  if command -v update-desktop-database >/dev/null 2>&1 && [ -d "$apps" ]; then
    update-desktop-database -q "$apps" || true
  fi
  if command -v gtk-update-icon-cache >/dev/null 2>&1 && [ -d "$icons" ]; then
    gtk-update-icon-cache -q -t -f "$icons" || true
  fi
}
