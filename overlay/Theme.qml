pragma Singleton
import QtQuick
import Quickshell
import Quickshell.Io

Singleton {
  id: root

  readonly property string stateHome: Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state"
  readonly property string currentPath: stateHome + "/omarchy/current"
  readonly property string themePath: currentPath + "/theme"
  readonly property string colorsPath: themePath + "/colors.toml"
  readonly property var fallback: ({
    background: "#121212", foreground: "#e5e2e1", accent: "#39ff14",
    cyan: "#8fae9a", muted: "#494543", green: "#86d27a",
    yellow: "#ffb347", red: "#ff5544", magenta: "#c4a7e7"
  })
  property string mode: "dark"
  property var palette: ({})
  property color background: fallback.background
  property color foreground: fallback.foreground
  property color primary: fallback.accent
  property color secondary: fallback.cyan
  property color border: fallback.muted
  property color success: fallback.green
  property color warning: fallback.yellow
  property color error: fallback.red
  property color magenta: fallback.magenta
  readonly property color highlight: Qt.tint(background, Qt.alpha(foreground, mode === "light" ? 0.07 : 0.05))
  readonly property color line: Qt.alpha(foreground, 0.12)
  readonly property color dim: readable(palette.muted || foreground, background, foreground)
  readonly property color badgeForeground: background
  readonly property string fontFamily: "monospace"

  // Ten colours for inputs, speakers and people, as "#rrggbb": the theme's own
  // made readable, then their hues turned when it has fewer. None is near
  // another or the accent, so no one reads as a selection.
  readonly property var inputPalette: distinct([
    secondary, magenta, palette.orange, success, error, palette.blue, warning,
    palette.bright_cyan, palette.bright_magenta, palette.bright_blue, palette.bright_green,
    palette.bright_red, palette.bright_yellow, palette.light_foreground, foreground
  ], 10)
  // Colours at least this far apart in RGB (0 to √3) read as different.
  readonly property real apart: 0.15

  function distinct(wanted, count) {
    const picked = []
    const values = [rgb(String(primary))]
    const take = color => {
      if (!color || picked.length === count)
        return
      const hex = String(readable(color, background, foreground))
      const value = rgb(hex)
      if (values.some(other => Math.hypot(value[0] - other[0], value[1] - other[1], value[2] - other[2]) < apart))
        return
      picked.push(hex)
      values.push(value)
    }
    wanted.forEach(take)
    const own = picked.slice()
    for (let turn = 1; turn < 12; turn++)
      for (const hex of own) {
        const value = rgb(hex)
        const color = Qt.rgba(value[0], value[1], value[2], 1)
        if (color.hslSaturation > 0.05)
          take(String(Qt.hsla((color.hslHue + turn / 12) % 1, color.hslSaturation, color.hslLightness, 1)))
      }
    return picked
  }

  function participantColor(index) {
    const tones = [primary, secondary, success, warning, magenta]
    return tones[index % tones.length]
  }

  FileView {
    id: colorsFile
    path: root.colorsPath
    watchChanges: true
    printErrors: false
    onFileChanged: root.scheduleReload()
    onLoaded: root.apply(text())
  }

  FileView {
    id: themeDirectory
    path: root.themePath
    watchChanges: true
    printErrors: false
    onFileChanged: root.scheduleReload()
  }

  FileView {
    path: root.currentPath
    watchChanges: true
    printErrors: false
    onFileChanged: root.scheduleReload()
  }

  Timer {
    id: reloadTimer
    interval: 100
    onTriggered: {
      colorsFile.path = ""
      themeDirectory.path = ""
      Qt.callLater(function() {
        themeDirectory.path = root.themePath
        colorsFile.path = root.colorsPath
        colorsFile.reload()
      })
    }
  }

  function scheduleReload() {
    reloadTimer.restart()
  }

  function rgb(value) {
    const match = String(value).match(/^#([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})$/)
    return match ? [1, 2, 3].map(i => parseInt(match[i], 16) / 255) : null
  }

  function luminance(color) {
    const channel = value => value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4)
    return 0.2126 * channel(color[0]) + 0.7152 * channel(color[1]) + 0.0722 * channel(color[2])
  }

  function contrast(a, b) {
    const first = luminance(a)
    const second = luminance(b)
    return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05)
  }

  function readable(wanted, base, ink) {
    const color = rgb(wanted)
    const backgroundColor = rgb(base)
    const foregroundColor = rgb(ink)
    if (!color || !backgroundColor || !foregroundColor)
      return ink
    for (let step = 0; step <= 20; step++) {
      const amount = step / 20
      const mixed = color.map((channel, index) => channel + (foregroundColor[index] - channel) * amount)
      if (contrast(mixed, backgroundColor) >= 4.5)
        return Qt.rgba(mixed[0], mixed[1], mixed[2], 1)
    }
    return ink
  }

  function apply(toml) {
    const colors = {}
    let themeMode = "dark"
    for (const line of toml.split("\n")) {
      const match = line.match(/^\s*(\w+)\s*=\s*"([^"]+)"/)
      if (match && match[1] === "mode")
        themeMode = match[2]
      else if (match && /^#[0-9a-fA-F]{6}$/.test(match[2]))
        colors[match[1]] = match[2]
    }
    mode = themeMode === "light" ? "light" : "dark"
    palette = colors
    background = colors.background || fallback.background
    foreground = colors.foreground || fallback.foreground
    primary = readable(colors.accent || fallback.accent, background, foreground)
    secondary = readable(colors.cyan || fallback.cyan, background, foreground)
    border = colors.muted || fallback.muted
    success = readable(colors.green || fallback.green, background, foreground)
    warning = readable(colors.yellow || fallback.yellow, background, foreground)
    error = readable(colors.red || fallback.red, background, foreground)
    magenta = readable(colors.magenta || fallback.magenta, background, foreground)
  }
}
