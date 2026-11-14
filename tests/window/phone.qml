import QtQuick
import Eco.Core

// The phone window, run by tests/window.rs: it says the daemon is reached and
// draws a trace for each input, with the level that arrives; once the daemon
// closes, it says it is down. Each check that fails is printed, and the exit
// status is how many failed.
QtObject {
  id: root

  property int failed: 0
  property Window phone: Qt.createComponent(Qt.resolvedUrl("../../overlay/phone.qml")).createObject(null)

  function check(name, ok) {
    if (!ok) {
      console.error("FAIL: " + name)
      failed += 1
    }
  }

  // Every item under `item` that `is` holds for.
  function findAll(item, is) {
    let found = []
    for (const child of item.children) {
      if (is(child))
        found.push(child)
      found = found.concat(findAll(child, is))
    }
    return found
  }
  function labels() { return findAll(phone.contentItem, item => item.visible && item.text !== undefined).map(item => item.text) }

  function state() { return findAll(phone.contentItem, item => item.objectName === "state")[0].section }

  property bool sent: false
  property Timer step: Timer {
    interval: 50
    repeat: true
    running: true
    onTriggered: {
      if (I18n.packs["en-US"] === undefined)
        return
      if (!root.sent && Eco.connected) {
        root.check("reached, it says connected", root.state() === I18n.t("phone.connected"))
        root.check("without inputs, it says so", root.labels().includes(I18n.t("phone.no_input")))
        Eco.inputs = [{ id: "mic" }]
        Eco.handle({ type: "signal", input: "mic", level: 0.5, speech: true })
        const traces = root.findAll(root.phone.contentItem, item => item.objectName === "trace")
        root.check("one trace per input", traces.length === 1)
        root.check("the trace follows the signal", traces.length === 1 && traces[0].source === Eco.signals.mic
          && traces[0].source.levels[0] === 0.5 && traces[0].source.speech)
        root.check("the input is named", root.labels().includes("MIC"))
        root.check("with an input, the empty note goes", !root.labels().includes(I18n.t("phone.no_input")))
        Eco.send("closing")
        root.sent = true
      } else if (root.sent && !Eco.connected) {
        root.check("down, it says offline", root.state() === I18n.t("phone.down"))
        Qt.exit(root.failed)
      }
    }
  }
}
