import QtQuick
import QtTest
import Eco.Core
import Eco.Kit

// The toast stack, run by tests/window.rs with a daemon socket that answers
// nothing: at most three show, a newer one pushing the oldest out but never
// the pinned one; information and success close after their time, errors
// stay; a stack too low hides the oldest but the pinned; the pointer over a
// toast or the keyboard on it holds it; Esc closes the toast with the
// keyboard; Tab reaches its close; and a notice of the same topic, or saying
// the same, replaces the older one. Each check that fails is printed, and the
// exit status is how many failed.
Window {
  id: root

  width: 600
  height: 600
  visible: true

  property int failed: 0
  property TestCase keys: null
  property var notices: []
  property int last: 0

  function check(name, ok) {
    if (!ok) {
      console.error("FAIL: " + name)
      failed += 1
    }
  }

  // Adds a notice of `kind`, pinned or not; its id.
  function add(kind, pinned) {
    notices = notices.concat([{ id: ++last, kind: kind, parts: [{ text: kind + " " + last }], pinned: pinned === true }])
    return last
  }
  function ids() { return notices.map(notice => notice.id) }
  function same(a, b) { return JSON.stringify(a) === JSON.stringify(b) }
  // The toast showing the notice `id`, or null.
  function toast(id) {
    return stack.contentItem.children.find(item => item.noticeId === id) || null
  }
  // The close of the toast showing `id`.
  function close(id) { return find(toast(id), item => String(item).startsWith("IconButton")) }
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
  // The notices Eco holds of `topic`.
  function ofTopic(topic) { return Eco.notices.filter(notice => notice.topic === topic) }

  TextInput { id: before; width: 100; height: 20; activeFocusOnTab: true }

  ToastStack {
    id: defaults
    visible: false
  }

  ToastStack {
    id: stack
    anchors { right: parent.right; bottom: parent.bottom; margins: 10 }
    timeout: 600
    notices: root.notices
    onDismissed: id => root.notices = root.notices.filter(notice => notice.id !== id)
  }

  // The checks, one per tick, each after what the one before set off settled.
  readonly property var steps: [
    () => {
      root.keys = Qt.createQmlObject("import QtTest; TestCase { when: false }", root.contentItem, "keys")
      root.check("toasts close after six seconds and show three at most", defaults.timeout === 6000 && defaults.max === 3)
      for (let i = 0; i < 4; i++)
        root.add("error")
    },
    () => {
      root.check("a fourth toast pushes the oldest out", root.same(root.ids(), [2, 3, 4]))
      root.check("three toasts show", stack.count === 3 && root.toast(1) === null)
      root.notices = []
      root.add("error", true)
      root.add("error")
      root.add("error")
      root.add("error")
    },
    () => {
      root.check("the pinned toast stays while a newer one pushes the oldest out", root.same(root.ids(), [5, 7, 8]))
      // Room for two toasts: the pinned one and the newest.
      stack.maxHeight = 2 * root.toast(8).height + 4
    },
    () => {
      root.check("a stack too low for its toasts shows the pinned and the newest",
        root.toast(5).visible && root.toast(8).visible && !root.toast(7).visible && stack.height < 3 * root.toast(5).height)
      stack.maxHeight = Infinity
    },
    () => {
      root.check("a stack with room again shows every toast", root.toast(7).visible)
      root.notices = []
      root.add("info")
      root.add("success")
      root.add("error")
    },
    () => root.check("toasts stay before their time", root.notices.length === 3),
    () => {
      root.check("information and success close after their time, errors stay",
        root.same(root.notices.map(notice => notice.kind), ["error"]))
      root.notices = []
      root.add("info")
      root.add("info")
    },
    () => {
      const toast = root.toast(12)
      root.keys.mouseMove(toast, toast.width / 2, toast.height / 2)
      root.close(13).forceActiveFocus()
    },
    () => root.check("held toasts stay past their time", root.same(root.ids(), [12, 13])),
    () => {
      root.check("toasts held by the pointer and the keyboard stay past their time", root.same(root.ids(), [12, 13]))
      root.keys.mouseMove(root.contentItem, 5, 5)
      before.forceActiveFocus()
    },
    () => root.check("a toast let go counts its time again from the start", root.same(root.ids(), [12, 13])),
    () => {
      root.check("a toast let go closes after its time", root.ids().length === 0)
      root.add("error")
      root.add("error")
    },
    () => {
      before.forceActiveFocus()
      root.keys.keyClick(Qt.Key_Tab)
      root.check("Tab reaches a toast's close", root.activeFocusItem === root.close(14))
      root.keys.keyClick(Qt.Key_Escape)
      root.check("Esc closes the toast with the keyboard, and only it", root.same(root.ids(), [15]))
      root.keys.mouseClick(root.close(15))
      root.check("the close closes its toast", root.ids().length === 0)
      Eco.post([{ text: "installing" }], "info", { topic: "test" })
      Eco.post([{ text: "installed" }], "success", { topic: "test" })
      const topic = root.ofTopic("test")
      root.check("a notice of the same topic replaces the older one", topic.length === 1 && topic[0].kind === "success")
      const held = Eco.notices.length
      Eco.tell([["status.saved"]], false)
      Eco.tell([["status.saved"]], false)
      root.check("a notice saying the same replaces the older one", Eco.notices.length === held + 1)
      Eco.say("broken", true)
      Eco.say("broken", false)
      root.check("a notice saying the same as one of another kind does not replace it",
        Eco.notices.filter(notice => notice.parts[0].text === "broken").length === 2)
      root.check("a done notice is a success", Eco.notices.some(notice => notice.kind === "success" && notice.parts[0].key === "status.saved"))
    },
    () => {
      root.check("a daemon not reached is said in a pinned error", root.ofTopic("offline").length === 1 && root.ofTopic("offline")[0].pinned
        && root.ofTopic("offline")[0].kind === "error")
      Qt.exit(root.failed)
    }
  ]

  // Each tick is 400 ms: a toast's 600 ms run out in the second tick after it.
  Timer {
    property int at: 0
    interval: 400
    repeat: true
    running: true
    onTriggered: {
      try {
        root.steps[at++]()
      } catch (error) {
        root.check("step " + at + " runs: " + error, false)
        if (at === root.steps.length)
          Qt.exit(root.failed)
      }
    }
  }
}
