import QtQuick
import Qt.labs.platform
import Eco.Core

// The tray icon, as eco-window runs it while `[ui] tray` is on: a click opens
// the eco window or brings it forward, and the menu asks what `eco window` and
// `eco stop` ask (docs/design.md §12.7). Its icon carries a dot while a session
// records.
SystemTrayIcon {
  id: tray

  // What the tray draws comes from the sessions (`session`) and the errors:
  // on every connection it asks the daemon to leave out the events it draws
  // nothing from (`events`, docs/design.md §10).
  readonly property var unrendered: [
    "signal", "transcript_partial", "transcript", "transcription", "transcript_edited",
    "transcript_reassigned", "transcript_removed", "line_merged", "line_split", "diarized",
    "speaker_renamed", "suggestion_thinking", "suggestion_draft", "suggestion_start",
    "suggestion_delta", "suggestion_end", "suggestion_removed", "note", "note_removed",
    "translated", "session_translation", "session_timeline", "session_detail",
    "session_context", "session_cost", "session_speakers", "session_tags", "session_renamed",
    "session_deleted", "session_export", "session_opened", "sessions", "sessions_found",
    "attendees_changed", "people", "tags", "tag_renamed", "tag_deleted", "hook_started",
    "hook_sent", "import_date", "import_started", "import_progress", "import_done", "devices",
    "models", "keys", "omapass", "config", "config_pending", "config_saved", "update",
    "update_progress", "window_call", "window_focus", "window_share"
  ]
  function leaveOut() {
    if (Eco.connected)
      Eco.send("events " + JSON.stringify({ except: unrendered }))
  }
  Component.onCompleted: leaveOut()
  property Connections reconnected: Connections {
    target: Eco
    function onConnectedChanged() { tray.leaveOut() }
  }

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
