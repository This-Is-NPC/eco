import QtQuick
import Eco.Core

// The eco window's views and its settings made only while wanted, run by
// tests/window.rs: the sessions list and the people are made when first shown,
// the list kept after and the people let go once they have left; the settings
// are made when they open and let go when they close, their window kept. Each
// check that fails is printed, and the exit status is how many failed.
QtObject {
  id: root

  property int failed: 0
  property Window shell: Qt.createComponent(Qt.resolvedUrl("../../overlay/shell.qml")).createObject(null)

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
  // The view `name` of the eco window, or null while it is not made.
  function view(name) { return find(shell.contentItem, item => item.objectName === name).item }
  // The settings window shell.qml created, or null.
  function settings() {
    for (const child of shell.data)
      if (child.item && child.item.title === "eco · settings")
        return child.item
    return null
  }

  // The checks, one per tick, each after what the one before set off settled,
  // a view's leaving included.
  readonly property var steps: [
    () => {
      root.check("no sessions list before it is shown", root.view("history") === null)
      root.check("no people before they are shown", root.view("people") === null)
      root.check("no cost before it is shown", root.view("cost") === null)
      Eco.view = "history"
    },
    () => {
      const history = root.view("history")
      root.check("the sessions list is made when shown, active", history !== null && history.active)
      Eco.view = "people"
    },
    () => {
      const people = root.view("people")
      root.check("the people are made when shown, active", people !== null && people.active)
      let focused = root.shell.activeFocusItem
      while (focused !== null && focused !== people)
        focused = focused.parent
      root.check("the people's rows take the keyboard", people !== null && focused === people)
      const history = root.view("history")
      root.check("the sessions list is kept, inactive", history !== null && !history.active)
      Eco.view = "start"
    },
    () => {
      root.check("the people are let go once they have left", root.view("people") === null)
      Eco.configOpen = true
    },
    () => {
      const window = root.settings()
      root.check("the settings are made when they open", window !== null && window.visible && window.view !== null)
      Eco.configOpen = false
    },
    () => {
      const window = root.settings()
      root.check("the settings are let go when they close", window !== null && !window.visible && window.view === null)
      Eco.configOpen = true
    },
    () => {
      root.check("and made again when they open again", root.settings().view !== null)
      Qt.exit(root.failed)
    }
  ]
  property int step: 0
  property Timer tick: Timer {
    interval: 400
    repeat: true
    running: true
    onTriggered: {
      root.steps[root.step]()
      root.step += 1
    }
  }
}
