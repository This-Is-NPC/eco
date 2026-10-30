import QtQuick
import Qt.labs.platform
import EcoHost
import Eco.Core

// The tray icon as eco-window runs it, run by tests/window.rs: once it meets
// the daemon, its states are checked, each failed check printed; then it is
// clicked and each menu item chosen in order, and the exit status is how many
// checks failed.
QtObject {
  id: root

  property int failed: 0
  property SystemTrayIcon tray: Qt.createComponent(Qt.resolvedUrl("../../overlay/tray.qml")).createObject(null)

  function check(name, ok) {
    if (!ok) {
      console.error("FAIL: " + name)
      failed += 1
    }
  }

  property Timer step: Timer {
    interval: 50
    repeat: true
    running: true
    onTriggered: {
      if (!Eco.connected)
        return
      stop()
      const tray = root.tray
      root.check("idle says so", tray.tooltip === I18n.t("tray.idle"))
      root.check("idle has no dot", String(tray.icon.source).endsWith("/icons/tray.png"))
      const recording = { id: "s1", title: "Retro", kind: "meeting", state: "recording", running_since: null }
      Eco.handle({ type: "session", live: [{ id: "s0", title: "Old", kind: "meeting", state: "paused", running_since: null }, recording] })
      root.check("recording names the session", tray.tooltip === I18n.t("tray.recording", { title: "Retro" }))
      root.check("recording has the dot", String(tray.icon.source).endsWith("/icons/tray-recording.png"))
      Eco.handle({ type: "session", live: [] })
      root.check("ended, it is idle again", tray.tooltip === I18n.t("tray.idle"))

      root.check("the icon holds its menu only where it can", tray.menu === (Host.trayHoldsMenu ? tray.actions : null))
      tray.activated(SystemTrayIcon.Context)
      tray.activated(SystemTrayIcon.Trigger)
      for (const item of tray.actions.items)
        if (!item.separator)
          item.triggered()
      Qt.exit(root.failed)
    }
  }
}
