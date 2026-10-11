import QtQuick
import Qt.labs.platform
import Eco.Core

// The tray icon, as eco-window runs it while `[ui] tray` is on: a click opens
// the eco window or brings it forward, and the menu asks what `eco window` and
// `eco stop` ask (docs/design.md §12.7). Its icon carries a dot while a session
// records.
SystemTrayIcon {
  visible: true
  icon.source: Qt.resolvedUrl(Eco.recordingSession ? "icons/tray-recording.png" : "icons/tray.png")
  tooltip: Eco.recordingSession
    ? I18n.t("tray.recording", { title: Eco.titleOf(Eco.recordingSession) })
    : I18n.t("tray.idle")
  onActivated: reason => {
    if (reason === SystemTrayIcon.Trigger)
      Eco.callWindow("focus")
  }

  menu: Menu {
    MenuItem {
      text: I18n.t("tray.open")
      onTriggered: Eco.callWindow("focus")
    }
    MenuItem {
      text: I18n.t("tray.new_session")
      onTriggered: Eco.callWindow("new_session")
    }
    MenuItem {
      text: I18n.t("tray.settings")
      onTriggered: Eco.callWindow("config")
    }
    MenuSeparator {}
    MenuItem {
      text: I18n.t("tray.stop")
      onTriggered: Eco.stopDaemon()
    }
  }
}
