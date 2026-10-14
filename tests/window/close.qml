import QtQuick
import Eco.Core

// The eco window as eco-window runs it, run by tests/window.rs: the settings
// open with a new model not saved, then the eco window is closed as its close
// button closes it, which "closing" on stderr marks. The process must then exit.
QtObject {
  id: root

  property Window shell: Qt.createComponent(Qt.resolvedUrl("../../overlay/shell.qml")).createObject(null)

  // The settings window shell.qml created, or null.
  function settings() {
    for (const child of shell.data)
      if (child.item && child.item.title === "eco · settings")
        return child.item
    return null
  }

  // The settings open first; once they show, they change and the window closes.
  property Timer step: Timer {
    interval: 50
    repeat: true
    running: true
    onTriggered: {
      Eco.configOpen = true
      const window = root.settings()
      if (window === null || !window.visible)
        return
      stop()
      Eco.handle({ type: "config", devices: [], omapass: { installed: false, page: "" }, credential_manager: false,
        presets: { chat: [{ name: "LOCAL", values: { base_url: "http://127.0.0.1/v1" } }], transcription: [] },
        config: { participants: [], models: [], kinds: ["meeting"], actions: [], contexts: [], context: { files: [] }, ui: { language: "auto" },
          stt: { model: "", language: "auto", languages: ["auto"] }, llm: { model: "" } } })
      window.view.create(1)
      if (!window.dirty)
        return Qt.exit(4)
      console.error("closing")
      root.shell.close()
    }
  }
}
