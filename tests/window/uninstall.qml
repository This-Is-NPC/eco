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

  property Timer poll: Timer {
    interval: 50
    running: true
    repeat: true
    onTriggered: {
      if (!Eco.connected)
        return
      root.settings.page = root.settings.at.general
      const general = root.settings.sections[root.settings.at.general]
      const dialog = general.uninstallDialog
      root.check("the section lists UNINSTALL", general.rows(Eco.config || { ui: {} }).some(row => row.id === "uninstall"))
      root.check("the dialog is closed until asked", !dialog.visible)
      dialog.open()
      root.check("asking opens the dialog", dialog.visible)
      root.check("the data is kept by default", !dialog.everything && dialog.warning === "")
      dialog.leading[0].clicked()
      root.check("the data chip asks to delete it too", dialog.everything && dialog.warning !== "")
      dialog.close()
      dialog.open()
      root.check("opening again keeps the data", !dialog.everything)
      dialog.leading[0].clicked()
      // What the UNINSTALL button does: close, then confirm, with the choice kept.
      dialog.confirm()
      Eco.handle({ type: "uninstall", stage: "failed", everything: true, detail: "no systemd-run" })
      const told = Eco.notices[Eco.notices.length - 1]
      root.check("a removal that did not start says why", told.kind === "error"
        && I18n.t(told.parts[0].key, told.parts[0].args).indexOf("no systemd-run") >= 0)
      Qt.exit(root.failed)
    }
  }
}
