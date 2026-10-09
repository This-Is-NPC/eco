-- eco: overlay rules and shortcuts. One shortcut per action in ~/.config/eco/config.toml.
-- `eco setup` adds the line that loads the installed copy to ~/.config/hypr/bindings.lua.

local function send(command)
  return "echo '" .. command .. "' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/eco.sock"
end

-- Gives the keyboard to the eco window titled `title`.
local function focus(title)
  return "hyprctl dispatch 'hl.dsp.focus({ window = \"title:^" .. title .. "$\" })'"
end

-- Asks the newest eco window to make `call`, the daemon opening one when none
-- is, then focuses the window it opened, so typing goes to eco and not to the
-- app that had it.
local function open(call, title)
  return send('window.call {"call":"' .. call .. '"}') .. " && " .. focus(title)
end

o.bind("SUPER + ALT + 1", "eco: ask", send("action ask"))
o.bind("SUPER + ALT + 2", "eco: probe", send("action probe"))
o.bind("SUPER + ALT + C", "eco: configuração", open("config", "eco · configuração"))
o.bind("SUPER + ALT + N", "eco: nova nota", open("new_session", "eco"))
o.bind("SUPER + ALT + P", "eco: pausar/retomar", send("session.toggle"))
o.bind("SUPER + ALT + H", "eco: notas", open("sessions", "eco"))
o.bind("SUPER + ALT + E", "eco: focar", focus("eco"))

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
