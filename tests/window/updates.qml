import QtQuick
import Eco.Core
import Eco.Settings

// The General section refreshes stale updates through the daemon socket and
// shows how old a fresh result is.
Item {
  id: root
  property int failed: 0
  property int step: 0
  property SettingsView settings: SettingsView {}

  function check(name, ok) {
    if (!ok) {
      console.error("FAIL: " + name)
      failed += 1
    }
  }

  Timer {
    interval: 50
    running: true
    repeat: true
    onTriggered: {
      if (!Eco.connected)
        return
      if (root.step++ === 0) {
        Eco.handle({ type: "update", current: "0.8.0", latest: "0.8.0", newer: false,
          notes: "", checked_at: Date.now() / 1000 - 7200, check: true, error: "" })
        root.settings.page = root.settings.at.general
        return
      }
      Eco.handle({ type: "update", current: "0.8.0", latest: "0.8.0", newer: false,
        notes: "", checked_at: Date.now() / 1000, check: true, error: "" })
      root.settings.page = 0
      root.settings.page = root.settings.at.general
      Eco.handle({ type: "update", current: "0.8.0", latest: "0.8.0", newer: false,
        notes: "", checked_at: Date.now() / 1000 - 7200, check: true, error: "" })
      root.check("a fresh result gives its age", root.settings.sections[root.settings.at.general].versionText === "0.8.0 · up to date · checked 2 h ago")
      Qt.exit(root.failed)
    }
  }
}
