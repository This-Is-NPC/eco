-- eco: overlay rules and shortcuts. One shortcut per action in ~/.config/eco/config.toml.
-- `eco setup` adds the line that loads the installed copy to ~/.config/hypr/bindings.lua.

-- Each key runs the eco CLI, which hands the request to the running daemon. Those
-- that open a view go to the newest eco window, the daemon opening one when none
-- is, and give it the keyboard, so typing goes to eco and not to the app that had it.
o.bind("SUPER + ALT + 1", "eco: ask", "eco window action ask")
o.bind("SUPER + ALT + 2", "eco: probe", "eco window action probe")
o.bind("SUPER + ALT + C", "eco: configuração", "eco window config")
o.bind("SUPER + ALT + N", "eco: nova nota", "eco window new")
o.bind("SUPER + ALT + P", "eco: pausar/retomar", "eco window toggle")
o.bind("SUPER + ALT + H", "eco: notas", "eco window sessions")
o.bind("SUPER + ALT + E", "eco: focar", "eco window focus")

-- The overlay floats on every workspace, keeps the size you give it, does not
-- takes focus when it opens, and opts out of Omarchy's default opacity so text
-- stays crisp over anything. It is not hidden from captures: Hyprland cannot
-- tell a screen share from a screenshot, so share a window, not the screen.
o.window({ class = "^eco$", title = "^eco$" }, {
  tag = "-default-opacity",
  opacity = "1 1",
  float = true,
  pin = true,
  size = "720 680",
  persistent_size = true,
})

-- The config window floats; pinned like the overlay, so it opens above it. It
-- keeps the size it asks for, which fits its screen, and centres on the screen
-- rather than on the overlay.
o.window({ class = "^eco$", title = "^eco · configuração$" }, {
  tag = "-default-opacity",
  opacity = "1 1",
  float = true,
  pin = true,
  center = true,
})
