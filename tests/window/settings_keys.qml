import QtQuick
import QtTest
import Eco.Core
import Eco.Settings

// The settings window's section keys, pressed as a keyboard presses them, run
// by tests/window.rs: a section's number chooses it unless a field being
// written holds the keyboard, Ctrl with it chooses it anywhere, and the rail
// unfolds while the keyboard is on it; a table's row commands are there and off
// with no row picked, Ctrl+E writes the row picked, and Esc then goes back to
// the table with it still picked. The rail keeps its order whatever is chosen, 0 is the tenth section, Ctrl+Tab
// and Ctrl+Shift+Tab wrap round the ends. The sections come in their order, and on
// General Enter on a number starts writing it and Esc puts it back. Each check
// that fails is printed, and the exit status is how many failed.
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
  property Item field: null
  function written() { return find(window.contentItem, item => item.visible && item.cursorPosition !== undefined && !item.readOnly) }
  function rail() { return find(window.contentItem, item => String(item).startsWith("TabRail")) }

  // The rail's tabs sit one under the other in the order of their numbers.
  function railInOrder() {
    const tabs = rail().children.filter(item => item.lit !== undefined)
    return tabs.length === 11 && tabs.every(tab => tab.y === tab.index * (rail().tabHeight + rail().gap))
  }

  // Keys reach the window once it is active and its rail has settled.
  function settled() { return window.active && rail() !== null && railInOrder() }

  // The checks, each after what the one before set off settled: a step returns
  // the condition the next one waits for.
  readonly property var steps: [
    () => {
      Eco.handle({ type: "config", devices: [], omapass: { installed: false, page: "" }, credential_manager: false,
        platform: "linux", presets: { secrets: [], providers: [{ name: "LOCAL", base_url: "http://127.0.0.1/v1" }], models: [] },
        config: { participants: [], secrets: [], providers: [], models: [], kinds: ["meeting"], actions: [], contexts: [], context: { files: [] }, ui: { language: "auto" },
          stt: { model: "", language: "auto", languages: ["auto"] }, llm: { model: "", max_context_chars: 60000, concurrency: 8 } } })
      Eco.configOpen = true
      root.keys = Qt.createQmlObject("import QtTest; TestCase { when: false }", root.window.contentItem, "keys")
      return root.settled
    },
    () => {
      root.window.choose(1)
      return () => root.window.page === 1 && root.settled()
    },
    () => {
      root.keys.keyClick(Qt.Key_3)
      root.check("3 on a section chooses the third", root.window.page === 2)
      return root.settled
    },
    () => {
      root.keys.keyClick(Qt.Key_5, Qt.ControlModifier)
      root.check("Ctrl+5 chooses the fifth", root.window.page === 4)
      return root.settled
    },
    () => {
      root.keys.keyClick(Qt.Key_1, Qt.KeypadModifier)
      root.check("the keypad's 1 chooses the first", root.window.page === 0)
      return root.settled
    },
    () => {
      root.keys.keyClick(Qt.Key_Tab, Qt.ControlModifier)
      root.check("Ctrl+Tab turns to the next", root.window.page === 1)
      return root.settled
    },
    () => {
      root.window.view.create(root.window.view.at.models)
      return () => root.written() !== null
    },
    () => {
      root.field = root.written()
      root.field.forceActiveFocus()
      return () => root.window.activeFocusItem === root.field
    },
    () => {
      const field = root.field
      const before = field.text
      root.keys.keyClick(Qt.Key_3)
      root.check("3 in a field is written there", root.window.page === root.window.view.at.models && field.text === before + "3")
      root.keys.keyClick(Qt.Key_5, Qt.ControlModifier)
      root.check("Ctrl+5 in a field chooses the fifth", root.window.page === 4)
      return root.settled
    },
    () => {
      const rail = root.rail()
      root.check("the rail is folded", !rail.unfolded)
      for (let presses = 0; presses < 20 && rail.focusedTabs === 0; presses++)
        root.keys.keyClick(Qt.Key_Backtab, Qt.ShiftModifier)
      return () => root.rail().unfolded
    },
    () => {
      const rail = root.rail()
      root.check("the rail unfolds with the keyboard on it", rail.unfolded)
      root.keys.keyClick(Qt.Key_Home)
      root.keys.keyClick(Qt.Key_Down)
      root.keys.keyClick(Qt.Key_Return)
      root.check("Home, Down and Enter on the rail choose the second", root.window.page === 1)
      root.window.choose(root.window.view.at.kinds)
      return root.settled
    },
    () => {
      const view = root.window.view
      view.picked = ({})
      const commands = view.tableCommands(view.at.kinds)
      const off = key => {
        const command = commands.find(entry => entry.text === I18n.t(key))
        return !!command && command.shown !== false && command.enabled === false && command.tip === I18n.t("settings.pick_row_tip")
      }
      root.check("with no row picked, VIEW is there and off", off("settings.view"))
      root.check("with no row picked, EDIT is there and off", off("settings.edit"))
      root.check("with no row picked, DEFAULT is there and off", off("settings.default"))
      root.check("with no row picked, DELETE is there and off", off("settings.delete"))
      view.picked = view.setOf(view.picked, view.at.kinds, "meeting")
      return root.settled
    },
    () => {
      root.keys.keyClick(Qt.Key_E, Qt.ControlModifier)
      return () => root.window.view.openPage !== null && root.window.view.openPage.editing
    },
    () => {
      const page = root.window.view.openPage
      root.check("Ctrl+E on the table opens the row picked to be written", page !== null && page.key === "meeting" && page.editing && page.returns)
      root.keys.keyClick(Qt.Key_Escape)
      return () => root.window.view.openPage === null
    },
    () => {
      const view = root.window.view
      root.check("Esc while writing goes back to the table", view.openPage === null)
      root.check("the row stays picked", view.picked[view.at.kinds] === "meeting")
      const names = ["audio", "secrets", "providers", "models", "languages", "kinds", "prompts", "context", "skills", "general", "appearance"]
      root.check("the sections come in their order", JSON.stringify(view.sections.map(section => section.name))
        === JSON.stringify(names.map(name => I18n.t("settings." + name))))
      root.check("each section has its place", names.every((name, index) => view.at[name] === index))
      root.window.choose(view.at.general)
      return root.settled
    },
    () => {
      const view = root.window.view
      view.picked = view.setOf(view.picked, view.at.general, "concurrency")
      return () => view.sections[view.at.general].note === I18n.t("general.concurrency_tip") && root.settled()
    },
    () => {
      const view = root.window.view
      root.check("the note says what the row picked does", view.sections[view.at.general].note === I18n.t("general.concurrency_tip"))
      root.keys.keyClick(Qt.Key_Return)
      const field = root.window.activeFocusItem
      root.check("Enter on a number starts writing it", field !== null && field.text === "8" && field.selectedText === "8")
      root.keys.keyClick(Qt.Key_3)
      root.check("what is typed replaces the number", field.text === "3")
      root.keys.keyClick(Qt.Key_Escape)
      root.check("Esc puts the number back", field.text === "8")
      root.check("Esc gives the keyboard back to the rows", root.window.activeFocusItem !== field)
    },
    () => {
      root.keys.keyClick(Qt.Key_9, Qt.ControlModifier)
      root.check("Ctrl+9 chooses the ninth", root.window.page === root.window.view.at.skills)
      root.keys.keyClick(Qt.Key_0)
      root.check("0 chooses the tenth", root.window.page === 9)
      root.check("the rail keeps its order", root.railInOrder())
      return root.settled
    },
    () => {
      root.keys.keyClick(Qt.Key_0, Qt.ControlModifier)
      root.check("Ctrl+0 chooses the tenth", root.window.page === 9)
      root.window.choose(10)
      root.check("choosing the eleventh lights the eleventh", root.window.page === 10)
      root.check("the rail keeps its order with the last chosen", root.railInOrder())
      return root.settled
    },
    () => {
      root.keys.keyClick(Qt.Key_Tab, Qt.ControlModifier)
      root.check("Ctrl+Tab from the last wraps to the first", root.window.page === 0)
      root.check("the rail keeps its order after the wrap", root.railInOrder())
      return root.settled
    },
    () => {
      root.keys.keyClick(Qt.Key_Backtab, Qt.ControlModifier | Qt.ShiftModifier)
      root.check("Ctrl+Shift+Tab from the first goes to the last", root.window.page === 10)
      return root.settled
    },
    () => {
      root.keys.keyClick(Qt.Key_Backtab, Qt.ControlModifier | Qt.ShiftModifier)
      root.check("Ctrl+Shift+Tab goes to the one before", root.window.page === 9)
      for (const [digit, page] of [[Qt.Key_4, 3], [Qt.Key_9, 8], [Qt.Key_2, 1], [Qt.Key_7, 6], [Qt.Key_1, 0]]) {
        root.keys.keyClick(digit)
        root.check("a digit chooses its section " + (page + 1), root.window.page === page)
      }
      root.check("the rail keeps its order after every digit", root.railInOrder())
      Qt.exit(root.failed)
    }
  ]

  property var ready: () => true
  property Timer step: Timer {
    property int at: 0
    interval: 10
    repeat: true
    running: true
    onTriggered: {
      if (!root.ready() || at >= root.steps.length)
        return
      // A key click runs the event loop, which can fire this timer again.
      const step = root.steps[at++]
      root.ready = () => false
      root.ready = step() || (() => true)
    }
  }
}
