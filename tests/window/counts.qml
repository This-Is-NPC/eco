import QtQuick
import Eco.Core
import Eco.Sessions

// A 6000-line session read back in one page, run by tests/window.rs: it loads and takes
// its translations within a time limit, its counts are the daemon's after a
// line is added, edited or removed and after another session is shown, and
// each check that fails is printed; the exit status is how many failed.
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

  // Whether an item under `item` shows a text holding `text`.
  function reads(item, text) {
    for (const child of item.children) {
      if (child.visible && typeof child.text === "string" && child.text.indexOf(text) >= 0)
        return true
      if (child.visible && reads(child, text))
        return true
    }
    return false
  }
  function counts(lines, answers, notes) { return { lines: lines, answers: answers, notes: notes } }
  function shown(kind, n) { return I18n.t(kind, { n: n }).toUpperCase() }
  // The row of the timeline heard at `at`, or null.
  function lineAt(at) {
    for (let i = 0; i < Eco.timeline.count; i++)
      if (Eco.timeline.get(i).kind === "speech" && Eco.timeline.get(i).at === at)
        return Eco.timeline.get(i)
    return null
  }

  readonly property double start: 1791083833.5
  // A stored session's summary, as `session.show` answers it.
  function stored(id, lines) {
    return {
      type: "session_detail", speakers: [],
      session: { id: id, title: id, kind: "meeting", source: "live", language: "en", state: "ended", started_at: start, duration_s: lines * 2,
                 path: "/tmp/" + id + ".jsonl", bytes: 1, people: [], attendees: [], tags: [], contexts: [], translating: null,
                 counts: counts(lines, Math.floor(lines / 120), 0) }
    }
  }
  // Its whole timeline in one page: the lines, an answer after every 120th.
  function page(id, lines) {
    const timeline = []
    for (let i = 0; i < lines; i++) {
      timeline.push({ type: "transcript", who: "Speaker " + (i % 3), name: "N" + (i % 3), text: "line " + i, at: start + i * 1.8 })
      if (i % 120 === 119)
        timeline.push({ type: "suggestion", id: "a" + i, action: "ask", model: "m", prompt: "ask", text: "answer " + i, at: start + i * 1.8 + 0.5, done: true })
    }
    return { type: "session_timeline", session: id, before: null, after: null, around: null, timeline: timeline,
             more_before: false, more_after: false }
  }
  // showStored opens a stored session as the daemon answers it.
  function showStored(id, lines) {
    Eco.showSession(id)
    Eco.handle(stored(id, lines))
    Eco.handle(page(id, lines))
  }

  SessionDetails {
    id: details
    width: root.width
    session: Eco.onScreen
    shown: true
  }

  Timer {
    interval: 50
    running: true
    onTriggered: {
      let began = Date.now()
      root.showStored("s1", 6000)
      const loaded = Date.now() - began
      root.check("6000 lines load in under 5 s, took " + loaded + " ms", loaded < 5000)
      root.check("the timeline holds them all", Eco.timeline.count === 6050)
      root.check("the counts are the daemon's", Eco.countOf("s1", "lines") === 6000 && Eco.countOf("s1", "answers") === 50 && Eco.countOf("s1", "notes") === 0)
      root.check("the details say the lines", root.reads(details, root.shown("history.speech", 6000)))

      // Each line and answer translated finds its row through the index.
      began = Date.now()
      for (let i = 5999; i >= 0; i--) {
        Eco.handle({ type: "translated", session: "s1", language: "pt", text: "linha " + i, at: root.start + i * 1.8 })
        if (i % 120 === 119)
          Eco.handle({ type: "translated", session: "s1", language: "pt", id: "a" + i, text: "resposta " + i })
      }
      const translated = Date.now() - began
      root.check("6050 translations land in under 3 s, took " + translated + " ms", translated < 3000)
      root.check("the first line has its translation", Eco.timeline.get(0).translation === "linha 0")
      root.check("the last line has its translation", root.lineAt(root.start + 5999 * 1.8).translation === "linha 5999")
      root.check("an answer has its translation", Eco.timeline.get(120).key === "a119" && Eco.timeline.get(120).translation === "resposta 119")
      Eco.handle({ type: "translated", session: "s1", language: "pt", text: "none", at: 1.0 })
      root.check("a translation of a line not held changes nothing", Eco.timeline.count === 6050)

      // Added: the line comes and the counts are the ones it carries.
      const late = root.start + 6000 * 1.8
      Eco.handle({ type: "transcript", session: "s1", who: "Speaker 1", name: "N1", text: "late", at: late, counts: root.counts(6001, 50, 0) })
      root.check("a line added comes last", Eco.timeline.get(Eco.timeline.count - 1).text === "late")
      root.check("a line added counts", Eco.countOf("s1", "lines") === 6001 && root.reads(details, root.shown("history.speech", 6001)))
      Eco.handle({ type: "note", session: "s1", id: "n1", text: "a note", at: late + 1, counts: root.counts(6001, 50, 1) })
      root.check("a note added counts", Eco.countOf("s1", "notes") === 1 && root.reads(details, root.shown("details.notes", 1)))

      // Edited: the row changes, the counts do not.
      const middle = root.start + 3000 * 1.8
      Eco.handle({ type: "transcript_edited", session: "s1", who: "Speaker 0", at: middle, text: "edited" })
      root.check("a line edited changes its row", root.lineAt(middle).text === "edited")
      root.check("a line edited does not count", Eco.countOf("s1", "lines") === 6001)
      Eco.handle({ type: "transcript_edited", session: "s1", who: "Speaker 2", at: middle, text: "not this" })
      root.check("an edit of another speaker's line at that time changes nothing", root.lineAt(middle).text === "edited")

      // Removed: the row goes, the rows after it are still found.
      Eco.handle({ type: "transcript_removed", session: "s1", who: "Speaker 0", at: middle, counts: root.counts(6000, 50, 1) })
      root.check("a line removed leaves the timeline", root.lineAt(middle) === null && Eco.timeline.count === 6051)
      root.check("a line removed counts", Eco.countOf("s1", "lines") === 6000 && root.reads(details, root.shown("history.speech", 6000)))
      const after = root.start + 3001 * 1.8
      Eco.handle({ type: "translated", session: "s1", language: "pt", text: "found", at: after })
      root.check("a line after one removed is still found", root.lineAt(after).translation === "found")
      Eco.handle({ type: "transcript_reassigned", session: "s1", who: "Speaker 1", at: after, label: "Speaker 1#p", name: "Ana" })
      root.check("a line after one removed is reassigned", root.lineAt(after).name === "Ana")
      Eco.handle({ type: "suggestion_removed", session: "s1", id: "a119", counts: root.counts(6000, 49, 1) })
      root.check("an answer removed leaves the timeline", Eco.locate("a119") === null && Eco.countOf("s1", "answers") === 49)
      root.check("an answer after it is still found", Eco.locate("a239") !== null && Eco.timeline.get(Eco.locate("a239").index).key === "a239")
      Eco.handle({ type: "note_removed", session: "s1", id: "n1", counts: root.counts(6000, 49, 0) })
      root.check("a note removed counts", Eco.locate("n1") === null && Eco.countOf("s1", "notes") === 0)
      // A session not on screen keeps its own counts.
      Eco.handle({ type: "transcript", session: "other", who: "Speaker 1", name: "N1", text: "elsewhere", at: late + 2, counts: root.counts(7, 0, 0) })
      root.check("a line of another session does not reach this one", Eco.timeline.get(Eco.timeline.count - 1).text === "late" && Eco.countOf("s1", "lines") === 6000)
      root.check("another session's counts are kept", Eco.countOf("other", "lines") === 7)

      // Switched: another session shown brings its own counts.
      root.showStored("s2", 3)
      root.check("another session shown brings its rows", Eco.timeline.count === 3)
      root.check("another session shown brings its counts", Eco.countOf("s2", "lines") === 3 && root.reads(details, root.shown("history.speech", 3)))
      Eco.handle({ type: "translated", session: "s2", language: "pt", text: "tres", at: root.start + 2 * 1.8 })
      root.check("its rows are found", Eco.timeline.get(2).translation === "tres")
      Qt.exit(root.failed)
    }
  }
}
