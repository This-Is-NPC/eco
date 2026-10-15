import QtQuick
import QtTest
import Eco.Core
import Eco.Kit
import Eco.Live

// A session's lines clicked and keyed as a user does, run by tests/window.rs,
// which reads what the window sends: a click, the line's "more" button and the
// Menu key open a line's menu, and a line being split has no button; MERGE keeps the line outlined until the next line clicked merges into
// it, and Esc or the same line again gives up; SPLIT opens the line with its
// cursor, and its confirm splits there, counting characters; the daemon's
// line_merged and line_split change the lines shown and the counts. Each check that fails is
// printed, and the exit status is how many failed.
Window {
  id: root

  width: 480
  height: 700
  visible: true

  property int failed: 0
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
  function kind(item, name) { return String(item).startsWith(name) }
  // The line on screen saying `text`.
  function turn(text) { return find(view, item => kind(item, "SpeechTurn") && item.text === text) }
  // The "more" button of the line `line`.
  function more(line) { return find(line, item => kind(item, "IconButton") && item.name === "more") }
  readonly property var menu: view.data.find(item => kind(item, "MenuPopup"))
  readonly property Item list: find(view, item => kind(item, "QQuickListView"))
  // The menu's entry the keyboard is on.
  function onEntry() { return menu.entries[menu.current] }
  // Moves the menu's keyboard to the entry `id` and picks it.
  function choose(id) {
    while (onEntry().id !== id)
      keys.keyClick(Qt.Key_Down)
    keys.keyClick(Qt.Key_Return)
  }

  TimelineView {
    id: view
    anchors.fill: parent
    sessionId: "s1"
  }

  readonly property var steps: [
    () => {
      root.keys = Qt.createQmlObject("import QtTest; TestCase { when: false }", root.contentItem, "keys")
      Eco.shown = "s1"
      Eco.handle({ type: "session", live: [{ id: "s1", title: "Call", kind: "meeting", source: "live", state: "recording", running_since: null }] })
    },
    () => {
      // The newest page the window asked for, as the daemon answers it.
      const said = [["Eles", "Bom dia.", 100], ["Eu", "Tudo bem?", 101], ["Eles", "Vamos começar agora", 102], ["Eles", "Oi 😀 tudo bem", 103]]
      Eco.handle({ type: "session_timeline", session: "s1", before: null, after: null, around: null, more_before: false, more_after: false,
                   timeline: said.map(([who, text, at]) => ({ type: "transcript", who: who, name: who, text: text, at: at })) })
    },
    () => {
      root.check("the window reached the daemon's socket", Eco.connected)
      root.check("the lines show", root.turn("Bom dia.") !== null && root.turn("Oi 😀 tudo bem") !== null)
      root.keys.mouseClick(root.turn("Bom dia.").box)
    },
    () => {
      root.check("a click on a line opens its menu", root.menu.opened && root.menu.line === root.turn("Bom dia."))
      root.check("the menu lists every line action",
        JSON.stringify(root.menu.entries.map(entry => entry.id)) === JSON.stringify(["edit", "line_speaker", "speaker", "copy", "merge", "split", "remove"]))
      root.keys.keyClick(Qt.Key_Escape)
    },
    () => {
      root.check("Esc closes the menu", !root.menu.opened)
      const button = root.more(root.turn("Tudo bem?"))
      root.check("a line shows its more button", button !== null && button.visible)
      if (button)
        root.keys.mouseClick(button)
    },
    () => {
      root.check("the more button opens its line's menu", root.menu.opened && root.menu.line === root.turn("Tudo bem?"))
      root.keys.keyClick(Qt.Key_Escape)
    },
    () => {
      root.check("Esc closes the menu", !root.menu.opened)
      root.list.forceActiveFocus()
      root.keys.keyClick(Qt.Key_Menu)
    },
    () => {
      root.check("the Menu key opens the menu of the line the keyboard is on", root.menu.opened && root.menu.line === root.turn("Oi 😀 tudo bem"))
      root.choose("merge")
    },
    () => {
      root.check("MERGE keeps the line outlined", view.merging !== null && root.turn("Oi 😀 tudo bem").merging)
      root.check("a strip says to pick the line to merge", root.find(view, item => item.text === I18n.t("line.merge_hint") && item.visible) !== null)
      root.keys.keyClick(Qt.Key_Escape)
      root.check("Esc gives the merge up", view.merging === null && !root.turn("Oi 😀 tudo bem").merging)
      root.keys.mouseClick(root.turn("Bom dia.").box)
    },
    () => {
      root.choose("merge")
    },
    () => {
      root.keys.mouseClick(root.turn("Bom dia.").box)
      root.check("the same line again gives the merge up", view.merging === null && !root.menu.opened)
      root.keys.mouseClick(root.turn("Bom dia.").box)
    },
    () => root.choose("merge"),
    () => {
      // Sent: session.line.merge into Eles at 100 from Eu at 101.
      root.keys.mouseClick(root.turn("Tudo bem?").box)
      root.check("the next line clicked merges, without a menu", view.merging === null && !root.menu.opened)
      Eco.handle({ type: "line_merged", session: "s1", into: { who: "Eles", at: 100 }, from: { who: "Eu", at: 101 }, text: "Bom dia. Tudo bem?",
                   counts: { lines: 3, answers: 0, notes: 0 } })
      root.check("a merge shows one line", Eco.timeline.count === 3 && Eco.timeline.get(0).text === "Bom dia. Tudo bem?")
      root.check("a merge's counts are the daemon's", Eco.countOf("s1", "lines") === 3)
      root.keys.mouseClick(root.turn("Vamos começar agora").box)
    },
    () => root.choose("split"),
    () => {
      const line = root.turn("Vamos começar agora")
      root.check("SPLIT opens the line with its cursor", line.editing && line.splitting)
      const button = root.more(line)
      root.check("a line being split hides its more button", button !== null && !button.visible)
      // Sent: session.line.split of Eles at 102 at character 6.
      root.keys.keyClick(Qt.Key_Home)
      for (let i = 0; i < 6; i++)
        root.keys.keyClick(Qt.Key_Right)
      root.keys.keyClick(Qt.Key_Return)
      root.check("the confirm closes the line", !line.editing && !line.splitting)
      Eco.handle({ type: "line_split", session: "s1", of: { who: "Eles", at: 102 }, text: "Vamos",
                   new: { who: "Eles", name: "Eles", at: 102.001, text: "começar agora" },
                   counts: { lines: 4, answers: 0, notes: 0 } })
      root.check("a split shows the right part right below",
        Eco.timeline.count === 4 && Eco.timeline.get(1).text === "Vamos" && Eco.timeline.get(2).text === "começar agora")
      root.check("a split's counts are the daemon's", Eco.countOf("s1", "lines") === 4)
      root.keys.mouseClick(root.turn("Oi 😀 tudo bem").box)
    },
    () => root.choose("split"),
    () => {
      // Sent: session.line.split of Eles at 103 at character 4, past the emoji.
      root.keys.keyClick(Qt.Key_Home)
      for (let i = 0; i < 4; i++)
        root.keys.keyClick(Qt.Key_Right)
      const confirm = root.find(root.turn("Oi 😀 tudo bem"), item => item.text === I18n.t("line.split") && item.visible)
      root.check("the split's confirm is named for it", confirm !== null)
      if (confirm)
        root.keys.mouseClick(confirm)
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
