import QtQuick
import QtQuick.Window
import Eco.Core
import Eco.Settings

// Settings › General has an UNINSTALL row whose dialog asks first, keeps the
// data unless it is turned on, and sends the command only when confirmed.
QtObject {
  id: root
  property int failed: 0
  // A window of its own: a dialog opens over one.
  property Window frame: Window {
    visible: true
    width: 700
    height: 600
    SettingsView {
      id: view
      anchors.fill: parent
    }
  }
  readonly property SettingsView settings: view

  function check(name, ok) {
    if (!ok) {
      console.error("FAIL: " + name)
      failed += 1
    }
  }

  readonly property var general: root.settings.sections[root.settings.at.general]
  readonly property var dialog: general.uninstallDialog

  // The checks, each after what the one before set off settled: a popup opens
  // only once its window is exposed and its transition has run.
  readonly property var steps: [
    () => {
      root.settings.page = root.settings.at.general
      root.check("the section lists UNINSTALL", root.general.rows(Eco.config || { ui: {} }).some(row => row.id === "uninstall"))
      root.check("the dialog is closed until asked", !root.dialog.visible)
      return () => root.frame.active
    },
    () => {
      root.dialog.open()
      return () => root.dialog.opened
    },
    () => {
      root.check("asking opens the dialog", root.dialog.visible)
      root.check("the data is kept by default", !root.dialog.everything && root.dialog.warning === "")
      root.dialog.leading[0].clicked()
      root.check("the data chip asks to delete it too", root.dialog.everything && root.dialog.warning !== "")
      root.dialog.close()
      return () => !root.dialog.visible
    },
    () => {
      root.dialog.open()
      return () => root.dialog.opened
    },
    () => {
      root.check("opening again keeps the data", !root.dialog.everything)
      root.dialog.leading[0].clicked()
      // What the UNINSTALL button does: close, then confirm, with the choice kept.
      root.dialog.confirm()
      Eco.handle({ type: "uninstall", stage: "failed", everything: true, detail: "no systemd-run" })
      const told = Eco.notices[Eco.notices.length - 1]
      root.check("a removal that did not start says why", told.kind === "error"
        && I18n.t(told.parts[0].key, told.parts[0].args).indexOf("no systemd-run") >= 0)
      Qt.exit(root.failed)
    }
  ]

  property var ready: () => Eco.connected
  property int at: 0
  property Timer poll: Timer {
    interval: 10
    running: true
    repeat: true
    onTriggered: {
      if (root.ready() && root.at < root.steps.length)
        root.ready = root.steps[root.at++]() || (() => true)
    }
  }
}
