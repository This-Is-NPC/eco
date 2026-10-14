import QtQuick
import QtTest
import Eco.Core
import Eco.Settings

// The settings window's section keys, pressed as a keyboard presses them, run
// by tests/window.rs: a section's number chooses it unless a field being
// written holds the keyboard, Ctrl with it chooses it anywhere, and the rail
// unfolds while the keyboard is on it. Each check that fails is printed, and
// the exit status is how many failed.
QtObject {
  id: root

  property int failed: 0
  property ConfigWindow window: ConfigWindow {}
  property TestCase keys: null

  function check(name, ok) {
    if (!ok) {
      console.error("FAIL: " + name)
      failed += 1
    }
  }

  // The first item under `item` that `is` holds for.
  function find(item, is) {
    for (const child of item.children) {
      if (is(child))
        return child
      const found = find(child, is)
      if (found)
        return found
    }
    return null
  }
  function rail() { return find(window.contentItem, item => String(item).startsWith("TabRail")) }

  // The checks, one per tick, each after what the one before set off settled.
  readonly property var steps: [
    () => {
      Eco.handle({ type: "config", devices: [], omapass: { installed: false, page: "" }, credential_manager: false,
        presets: { chat: [{ name: "LOCAL", values: { base_url: "http://127.0.0.1/v1" } }], transcription: [{ name: "LOCAL", values: { base_url: "http://127.0.0.1/v1" } }] },
        config: { participants: [], models: [], kinds: ["meeting"], actions: [], contexts: [], context: { files: [] }, ui: { language: "auto" },
          stt: { model: "", language: "auto", languages: ["auto"] }, llm: { model: "" } } })
      Eco.configOpen = true
      root.keys = Qt.createQmlObject("import QtTest; TestCase { when: false }", root.window.contentItem, "keys")
    },
    () => root.window.choose(1),
    () => {
      root.keys.keyClick(Qt.Key_3)
      root.check("3 on a section chooses the third", root.window.page === 2)
    },
    () => {
      root.keys.keyClick(Qt.Key_5, Qt.ControlModifier)
      root.check("Ctrl+5 chooses the fifth", root.window.page === 4)
    },
    () => {
      root.keys.keyClick(Qt.Key_1, Qt.KeypadModifier)
      root.check("the keypad's 1 chooses the first", root.window.page === 0)
    },
    () => {
      root.keys.keyClick(Qt.Key_Tab, Qt.ControlModifier)
      root.check("Ctrl+Tab turns to the next", root.window.page === 1)
    },
    () => root.window.view.create(1),
    () => {
      root.find(root.window.contentItem, item => item.visible && item.cursorPosition !== undefined && !item.readOnly).forceActiveFocus()
    },
    () => {
      const field = root.window.activeFocusItem
      const before = field.text
      root.keys.keyClick(Qt.Key_3)
      root.check("3 in a field is written there", root.window.page === 1 && field.text === before + "3")
      root.keys.keyClick(Qt.Key_5, Qt.ControlModifier)
      root.check("Ctrl+5 in a field chooses the fifth", root.window.page === 4)
    },
    () => {
      const rail = root.rail()
      root.check("the rail is folded", !rail.unfolded)
      for (let presses = 0; presses < 20 && rail.focusedTabs === 0; presses++)
        root.keys.keyClick(Qt.Key_Backtab, Qt.ShiftModifier)
    },
    () => {
      const rail = root.rail()
      root.check("the rail unfolds with the keyboard on it", rail.unfolded)
      root.keys.keyClick(Qt.Key_Home)
      root.keys.keyClick(Qt.Key_Down)
      root.keys.keyClick(Qt.Key_Return)
      root.check("Home, Down and Enter on the rail choose the one after the head", root.window.page === 5)
    },
    () => Qt.exit(root.failed)
  ]

  property Timer step: Timer {
    property int at: 0
    interval: 400
    repeat: true
    running: true
    onTriggered: root.steps[at++]()
  }
}
