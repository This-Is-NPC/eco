import QtQuick
import Eco.Core
import Eco.Live
import Eco.Settings

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

  // An item of the settings with nothing related, and one with a link.
  ItemPage {
    id: alone
    width: root.width
    height: 300
    title: "GENERAL"
    facts: [["language", "English"]]
    related: [{ title: "Models", items: [] }]
  }
  ItemPage {
    id: linked
    width: root.width
    height: 300
    kind: "model"
    title: "m"
    facts: [["type", "chat"]]
    related: [{ title: "Session kinds", items: [{ section: 3, id: "meeting", text: "MEETING" }] }]
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

      // A capture error goes with the last recording session; others stay.
      const recording = { id: "s1", title: "Retro", kind: "meeting", source: "live", state: "recording", running_since: null }
      Eco.handle({ type: "session", live: [recording] })
      Eco.handle({ type: "error", code: "capture.stopped", params: { detail: "gone" }, message: "capture stopped: gone" })
      root.check("a capture error shows while the session records", Eco.message !== "")
      Eco.handle({ type: "session", live: [] })
      root.check("a capture error goes when the session ends", Eco.message === "")
      Eco.handle({ type: "session", live: [recording] })
      Eco.handle({ type: "error", code: "transcription.failed", params: { who: "Eles", detail: "x" }, message: "x" })
      Eco.leaveSession()
      root.check("a transcription error goes when the session is left", Eco.message === "")
      Eco.handle({ type: "error", code: "import.failed", params: { detail: "bad.mp4" }, message: "import: bad.mp4" })
      Eco.handle({ type: "session", live: [] })
      root.check("an import error stays when a session ends", Eco.message !== "")
      // RELATED shows only when something is related.
      root.check("an item with nothing related has no RELATED", !root.shows(alone, I18n.t("settings.related").toUpperCase()))
      root.check("an item with a link has RELATED", root.shows(linked, I18n.t("settings.related").toUpperCase()))
      Qt.exit(root.failed)
    }
  }
}
