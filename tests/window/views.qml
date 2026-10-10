import QtQuick
import Eco.Core
import Eco.Live

// Overlay views driven as the daemon drives them, run by tests/window.rs: each
// check that fails is printed, and the exit status is how many failed.
Item {
  id: root
  width: 420
  height: 600
  property int failed: 0

  function check(name, ok) {
    if (!ok) {
      console.error("FAIL: " + name)
      failed += 1
    }
  }

  // Whether an item under `item` shows `text`.
  function shows(item, text) {
    for (const child of item.children) {
      if (child.visible && child.text === text)
        return true
      if (child.visible && shows(child, text))
        return true
    }
    return false
  }

  SuggestionCard {
    id: card
    width: root.width
    suggestionId: "a1"
    text: ""
    action: "chat"
    modelName: "m"
    prompt: "which queue?"
    streaming: true
    thinking: ""
    failure: ""
    at: Date.now() / 1000
    stamp: "10:00:00"
  }

  Timer {
    interval: 50
    running: true
    onTriggered: {
      // The wait is told until the first word arrives, not while it streams.
      const waiting = I18n.t("card.waiting", { seconds: card.waited })
      root.check("the card waits before the first word", root.shows(card, waiting))
      card.text = "Kafka"
      root.check("the card no longer waits once words arrive", !root.shows(card, waiting))
      Qt.exit(root.failed)
    }
  }
}
