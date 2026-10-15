import QtQuick
import Eco.Core
import Eco.Live

// A long session's timeline read in pages, driven as the daemon drives it, run
// by tests/window.rs: no daemon listens, so what the window asks for is the
// page it waits for. Each check that fails is printed, and the exit status is
// how many failed.
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

  // `n` lines said a second apart, the first at `from` seconds.
  function lines(from, n) {
    const said = []
    for (let i = 0; i < n; i++)
      said.push({ type: "transcript", who: "Eles", name: "Eles", text: "line " + (from + i), at: from + i })
    return said
  }

  function page(session, asked, entries, moreBefore, moreAfter) {
    return Object.assign({ type: "session_timeline", session: session, before: null, after: null, around: null,
      timeline: entries, more_before: moreBefore, more_after: moreAfter }, asked)
  }

  function asked(page) {
    return JSON.stringify(Eco.timelineAsked) === JSON.stringify(Object.assign({ id: "", before: null, after: null, around: null, limit: 300 }, page))
  }

  // The index of the entry at the top of the view.
  function topEntry() {
    let index = -1
    for (let y = list.contentY + list.topMargin + 1; index < 0 && y < list.contentY + list.height; y += 4)
      index = list.indexAt(list.width / 2, y)
    return index
  }

  TimelineView {
    id: view
    anchors.fill: parent
    model: Eco.timeline
    sessionId: "s1"
  }
  // The view's list, the child that follows the newest entry.
  property var list: null
  function listOf(item) {
    for (let i = 0; i < item.children.length; i++)
      if (item.children[i].following !== undefined)
        return item.children[i]
    return null
  }

  // Where the entry at the top was before a page came above it.
  property int held: -1
  property real heldOffset: 0

  property int step: 0
  readonly property var steps: [
    () => {
      root.list = root.listOf(view)
      Eco.handle({ type: "session", live: [{ id: "s1", title: "Long", kind: "meeting", state: "recording", active_s: 0, running_since: null }] })
      Eco.showSession("s1")
    },
    () => {
      root.check("a live session shown asks for its newest page", root.asked({ id: "s1" }))
      Eco.handle(root.page("s1", {}, root.lines(1700, 300), true, false))
      root.check("the newest page fills the timeline", Eco.timeline.count === 300 && Eco.moreBefore && !Eco.moreAfter)
      root.check("a page read back is not new", !Eco.timeline.get(0).fresh)
    },
    () => {
      // The user scrolls up to the top of what is held.
      list.following = false
      list.contentY = list.originY
    },
    () => {
      root.check("nearing the top asks for the page before the oldest entry held", root.asked({ id: "s1", before: 1700 }))
      root.held = root.topEntry()
      root.heldOffset = list.itemAtIndex(root.held).y - list.contentY
      Eco.handle(root.page("s1", { before: 1700 }, root.lines(1400, 300), true, false))
      root.check("the older page goes above", Eco.timeline.count === 600 && Eco.timeline.get(0).at === 1400)
      root.check("no other page is asked for at once", Eco.timelineAsked === null)
    },
    () => {
      const kept = list.itemAtIndex(root.held + 300)
      root.check("the entry read stays where it was once laid out", kept !== null && Math.abs(kept.y - list.contentY - root.heldOffset) < 1)
      root.check("far from the top, no other page is asked for", Eco.timelineAsked === null)
      // Back at the newest, following it, while the session goes on.
      list.positionViewAtEnd()
      list.following = true
      for (let i = 0; i < 450; i++)
        Eco.handle({ type: "transcript", session: "s1", who: "Eles", name: "Eles", text: "new " + i, at: 3000 + i })
      root.check("the live timeline keeps at most its cap", Eco.timeline.count === Eco.timelineCap)
      root.check("the oldest entries leave first", Eco.timeline.get(0).at === 1450 && Eco.moreBefore)
      root.check("a new entry is new", Eco.timeline.get(Eco.timeline.count - 1).fresh)
      // Reading older entries keeps them while lines come.
      list.following = false
      for (let i = 450; i < 460; i++)
        Eco.handle({ type: "transcript", session: "s1", who: "Eles", name: "Eles", text: "new " + i, at: 3000 + i })
      root.check("nothing leaves while the user reads", Eco.timeline.count === Eco.timelineCap + 10)
      // A line looked up indexes the rows held.
      Eco.handle({ type: "transcript_edited", session: "s1", who: "Eles", at: 3459, text: "newest edited" })
      list.following = true
      root.check("following the newest again lets the oldest go", Eco.timeline.count === Eco.timelineCap && Eco.timeline.get(0).at === 1460)
      root.check("the oldest leaving keeps the rows' index", Eco.rows !== null)
      // A change to a line applies only to a line held.
      Eco.handle({ type: "transcript_edited", session: "s1", who: "Eles", at: 1460, text: "edited" })
      root.check("a held line is edited", Eco.timeline.get(0).text === "edited")
      Eco.handle({ type: "transcript_edited", session: "s1", who: "Eles", at: 3455, text: "late edited" })
      root.check("a line is found after the oldest left", Eco.timeline.get(Eco.timeline.count - 5).text === "late edited")
      Eco.handle({ type: "transcript_removed", session: "s1", who: "Eles", at: 1401 })
      root.check("a line not held is not looked for", Eco.timeline.count === Eco.timelineCap)
    },
    () => {
      // A session found by a search opens where it was found.
      Eco.showSession("s2", 5050)
      root.check("a found session asks for the page around the time found", root.asked({ id: "s2", around: 5050 }))
    },
    () => {
      root.check("the page asked for survives the live session leaving the screen", root.asked({ id: "s2", around: 5050 }))
      Eco.handle({ type: "session_detail", session: { id: "s2", title: "Old", kind: "meeting", state: "ended", tags: [], people: [], attendees: [], contexts: [] }, speakers: [] })
      Eco.handle(root.page("s2", { around: 5050 }, root.lines(5000, 100), true, true))
      root.check("the page around the time fills the timeline", Eco.timeline.count === 100 && Eco.moreBefore && Eco.moreAfter)
      Eco.handle({ type: "note", session: "s2", id: "n1", text: "later", at: 9000 })
      root.check("a new entry does not go below a page short of the newest", Eco.timeline.count === 100)
    },
    () => {
      const middle = list.indexAt(list.width / 2, list.contentY + list.height / 2)
      root.check("the view shows the entry found, not the newest", !list.following && Math.abs(middle - 50) <= 3)
      list.positionViewAtEnd()
    },
    () => {
      root.check("nearing the end asks for the page after the newest entry held", root.asked({ id: "s2", after: 5099 }))
      Eco.handle(root.page("s2", { before: 4000 }, root.lines(3990, 10), false, true))
      root.check("a page not asked for is not put in", Eco.timeline.count === 100)
      Eco.handle(root.page("s2", { after: 5099 }, root.lines(5100, 50), true, false))
      root.check("the newer page goes below", Eco.timeline.count === 150 && Eco.timeline.get(149).at === 5149 && !Eco.moreAfter)
      Qt.exit(root.failed)
    }
  ]

  Timer {
    interval: 100
    running: true
    repeat: true
    onTriggered: if (root.step < root.steps.length) root.steps[root.step++]()
  }
}
