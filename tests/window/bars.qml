import QtQuick
import QtTest
import Eco.Core
import Eco.Kit
import Eco.Sessions
import Eco.Settings
import Eco.Window

// Bars and tables pressed as a keyboard and a pointer press them, run by
// tests/window.rs with a daemon socket that answers nothing: a row's own act
// takes its Enter and Space, a row without one opens on Space, live tools take
// Tab row by row, new rows keep the keyboard on the table, the guarded command
// stands behind a hairline, a bar's readout gives way when narrow, short names
// stand in for long ones before icons do, the
// sessions' NEW SESSION and N start a session, the details' close closes
// them, and a model's key picker lists each source's names under its title,
// its keyboard stepping over the titles, a pick setting that source's field. Each check that fails is printed, and the exit status is how many failed.
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
  // The model the key picker edits, changed as the model page changes it.
  property var model: ({ base_url: "https://openrouter.ai/api/v1", model: "m", api_key_env: "OPENROUTER_API_KEY" })

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
    id: tables
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

  ProviderFields {
    id: provider
    visible: false
    width: root.width
    target: "llm"
    section: root.model
    onEdited: (key, value) => {
      const model = Object.assign({}, root.model)
      if (value === null)
        delete model[key]
      else
        model[key] = value
      root.model = model
    }
  }
  readonly property Item picker: find(provider, item => kind(item, "Dropdown"))
  readonly property var pickerList: picker ? picker.data.find(item => kind(item, "MenuPopup")) : null
  // The key picker's entry the keyboard is on.
  function onKey() { return pickerList.entries[pickerList.current] }

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
      session.visible = false
      tables.visible = false
      provider.visible = true
      Eco.handle({ type: "keys", env: ["ECO_LAN_KEY", "OPENROUTER_API_KEY"], credentials: null, credential_error: "" })
      Eco.handle({ type: "omapass", installed: true, accounts: [{ account: "openrouter", folder: "AI" }] })
      root.picker.open()
    },
    () => {
      const titles = root.pickerList.entries.filter(entry => entry.heading !== undefined).map(entry => entry.heading)
      root.check("the key picker titles each source", JSON.stringify(titles) === JSON.stringify([I18n.t("provider.key_env"), I18n.t("provider.key_omapass")]))
      root.check("the key picker opens on the chosen key", root.onKey().text === "OPENROUTER_API_KEY")
      root.keys.keyClick(Qt.Key_Home)
      root.check("Home stops on the first name, past its title", root.onKey().text === "ECO_LAN_KEY")
      root.keys.keyClick(Qt.Key_Up)
      root.check("Up never lands on a title", root.onKey().text === "ECO_LAN_KEY")
      root.keys.keyClick(Qt.Key_Down)
      root.keys.keyClick(Qt.Key_Down)
      root.keys.keyClick(Qt.Key_Down)
      root.check("Down steps over the next source's title", root.onKey().text === "AI · openrouter")
      root.keys.keyClick(Qt.Key_Return)
      root.check("a picked omapass password is the model's key, and only it", root.model.api_key_omapass === "openrouter" && root.model.api_key_env === undefined)
    },
    () => {
      // The keyboard on another field as the list opens, which it goes back to unless a pick moves it.
      root.find(provider, item => item.text === "m" && item.cursorPosition !== undefined).forceActiveFocus()
      root.picker.open()
      root.keys.keyClick(Qt.Key_Home)
      root.keys.keyClick(Qt.Key_Return)
      root.check("a picked variable is the model's key, and only it", root.model.api_key_env === "ECO_LAN_KEY" && root.model.api_key_omapass === undefined)
    },
    () => {
      root.check("a pick gives the keyboard back to the field that had it", root.activeFocusItem !== null && root.activeFocusItem.text === "m")
      root.picker.open()
    },
    () => {
      const width = root.pickerList.width
      Eco.handle({ type: "keys", env: ["ECO_A_VERY_LONG_NAME_FOR_A_LAN_KEY", "ECO_LAN_KEY"], credentials: null, credential_error: "" })
      root.check("names listed while open widen the list", root.pickerList.width > width)
      root.check("names listed while open keep the keyboard on the chosen key", root.onKey().text === "ECO_LAN_KEY")
      root.keys.keyClick(Qt.Key_Down)
      root.keys.keyClick(Qt.Key_Return)
      root.check("TYPE A NAME… keeps the source with its name", root.model.api_key_env === "ECO_LAN_KEY")
    },
    () => {
      root.check("TYPE A NAME… gives the keyboard to the name once the list closed", root.activeFocusItem !== null && root.activeFocusItem.text === "ECO_LAN_KEY")
      root.picker.open()
    },
    () => {
      root.keys.keyClick(Qt.Key_End)
      root.keys.keyClick(Qt.Key_Return)
      root.check("TYPE A NAME… under OMAPASS starts an empty omapass name", root.model.api_key_omapass === "" && root.model.api_key_env === undefined)
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
