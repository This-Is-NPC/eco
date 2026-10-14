import QtQuick
import QtTest
import Eco.Core
import Eco.Kit
import Eco.Sessions
import Eco.Window

// Bars and tables pressed as a keyboard and a pointer press them, run by
// tests/window.rs with a daemon socket that answers nothing: a row's own act
// takes its Enter and Space, a row without one opens on Space, live tools take
// Tab row by row, new rows keep the keyboard on the table, the guarded command
// stands behind a hairline, a bar's readout gives way when narrow, short names
// stand in for long ones before icons do, the
// sessions' NEW SESSION and N start a session, and the details' close closes
// them. Each check that fails is printed, and the exit status is how many failed.
Window {
  id: root

  width: 720
  height: 900
  visible: true

  property int failed: 0
  property TestCase keys: null
  property var acted: []
  property var opened: []
  property var livePicked: []
  property int started: 0
  property bool detailsOpen: true

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
  // A hairline drawn under `item`.
  function hairline(item) { return find(item, child => child.width === 1 && child.height === 20 && drawn(child)) }
  function kind(item, name) { return String(item).startsWith(name) }
  // Whether `item` is drawn: it and every item around it visible and not clear.
  function drawn(item) {
    for (let at = item; at; at = at.parent)
      if (!at.visible || at.opacity === 0)
        return false
    return true
  }
  // Whether something under `item` drawn reads `text`.
  function reads(item, text) { return find(item, child => child.text === text && drawn(child)) !== null }

  Connections {
    target: Eco
    function onNewSessionRequested() { root.started += 1 }
  }

  Column {
    width: root.width

    DataTable {
      id: acting
      width: parent.width
      height: 140
      picks: true
      columns: [{ title: "name", weight: 1 }]
      rows: [{ id: "a" }, { id: "b" }]
      cells: entry => [entry.id]
      toolsAlways: true
      toolsWidth: 40
      tools: Item {
        property var entry: ({})
        function act() { root.acted = root.acted.concat([entry.id]) }
        implicitWidth: 30
        implicitHeight: 20
      }
      onOpened: entry => root.opened = root.opened.concat(["acting:" + entry.id])
    }
    DataTable {
      id: live
      width: parent.width
      height: 140
      picks: true
      columns: [{ title: "name", weight: 1 }]
      rows: [{ id: "d" }, { id: "e" }]
      cells: entry => [entry.id]
      toolsAlways: true
      toolsLive: true
      toolsWidth: 40
      tools: Item {
        property var entry: ({})
        implicitWidth: 30
        implicitHeight: 20
        Rectangle { objectName: "tool"; activeFocusOnTab: true; width: 30; height: 20 }
      }
      onPicked: entry => root.livePicked = root.livePicked.concat([entry.id])
    }
    DataTable {
      id: plain
      width: parent.width
      height: 140
      picks: true
      columns: [{ title: "name", weight: 1 }]
      rows: [{ id: "c" }]
      cells: entry => [entry.id]
      onOpened: entry => root.opened = root.opened.concat([entry.id])
    }
    CommandRow {
      id: mixed
      commands: [{ text: "VIEW", icon: "chevron-right", run: () => {} }, { text: "DELETE", icon: "trash", role: "guarded", run: () => {} }]
    }
    CommandRow {
      id: alone
      commands: [{ text: "DELETE", icon: "trash", role: "guarded", run: () => {} }]
    }
    PageBar {
      id: shortBar
      width: 520
      height: 44
      commands: [
        { text: "ADD", icon: "plus", role: "primary", run: () => {} },
        { text: "DEFAULT FOR CHAT", short: "FOR CHAT", icon: "check", run: () => {} },
        { text: "DEFAULT FOR TRANSLATION", short: "FOR TRANSLATION", icon: "check", run: () => {} },
        { text: "DEFAULT FOR REVIEW", short: "FOR REVIEW", icon: "check", run: () => {} }
      ]
    }
    PageBar {
      id: bar
      width: parent.width
      height: 44
      readout: "7 SESSIONS"
      commands: [
        { text: "HOME", role: "back", run: () => {} },
        { text: "NEW SESSION", icon: "plus", role: "primary", run: () => {} },
        { text: "IMPORT", icon: "import", run: () => {} }
      ]
    }
  }

  HistoryView {
    id: history
    visible: false
    width: root.width
    height: 400
  }
  SessionView {
    id: session
    visible: false
    width: root.width
    height: 500
    detailsOpen: root.detailsOpen
    onDetailsClosed: root.detailsOpen = false
  }

  // The checks, one per tick, each after what the one before set off settled.
  readonly property var steps: [
    () => root.keys = Qt.createQmlObject("import QtTest; TestCase { when: false }", root.contentItem, "keys"),
    () => acting.focusRows(),
    () => {
      root.keys.keyClick(Qt.Key_Return)
      root.keys.keyClick(Qt.Key_Down)
      root.keys.keyClick(Qt.Key_Space)
      root.check("Enter and Space run a row's own act", JSON.stringify(root.acted) === JSON.stringify(["a", "b"]))
      root.check("a row with its own act does not open", root.opened.length === 0)
      live.focusRows()
    },
    () => {
      root.keys.keyClick(Qt.Key_Tab)
      const first = root.activeFocusItem
      root.check("Tab from the rows reaches the picked row's tool", first !== null && first.objectName === "tool")
      root.keys.keyClick(Qt.Key_Tab)
      const second = root.activeFocusItem
      root.check("Tab walks on to the next row's tool, which keeps the keyboard", second !== null && second.objectName === "tool" && second !== first)
      root.check("the row a live tool is reached in is picked", root.livePicked[root.livePicked.length - 1] === "e")
      live.rows = [{ id: "d" }, { id: "e" }]
    },
    () => {
      let at = root.activeFocusItem
      while (at !== null && at !== live)
        at = at.parent
      root.check("new rows keep the keyboard on the table", at === live)
      plain.focusRows()
    },
    () => {
      root.keys.keyClick(Qt.Key_Space)
      root.check("Space opens a row with no act of its own", JSON.stringify(root.opened) === JSON.stringify(["c"]))
      root.check("a hairline stands before the guarded command", root.hairline(mixed) !== null)
      root.check("no hairline before a guarded command alone", root.hairline(alone) === null)
      root.check("the readout shows on a wide bar", root.reads(bar, "7 SESSIONS"))
      root.check("a tight bar shows the short names", root.reads(shortBar, "FOR CHAT") && !root.reads(shortBar, "DEFAULT FOR CHAT"))
      shortBar.width = 300
      bar.width = 330
    },
    () => {
      root.check("the readout gives way on a narrow bar", !root.reads(bar, "7 SESSIONS"))
      root.check("the narrow bar keeps NEW SESSION named", root.reads(bar, "NEW SESSION"))
      root.check("a bar too narrow for the short names shows icons", !root.reads(shortBar, "FOR CHAT") && root.reads(shortBar, "ADD"))
      history.visible = true
      history.active = true
      const sessions = root.find(history, item => root.kind(item, "PageBar"))
      sessions.commands.find(command => command.role === "primary").run()
      root.check("the sessions' NEW SESSION starts a session", root.started === 1)
    },
    () => {
      root.check("the window reached the daemon's socket", Eco.connected)
      root.keys.keyClick(Qt.Key_N)
      root.check("N on the sessions starts a session", root.started === 2)
      history.visible = false
      history.active = false
      session.visible = true
    },
    () => {
      const close = root.find(session, item => item.visible && item.name === "chevron-up" && root.kind(item, "IconButton"))
      root.check("the details have a close", close !== null)
      if (close)
        root.keys.mouseClick(close)
      root.check("the details' close closes them", !root.detailsOpen)
    },
    () => Qt.exit(root.failed)
  ]

  Timer {
    property int at: 0
    interval: 400
    repeat: true
    running: true
    onTriggered: root.steps[at++]()
  }
}
