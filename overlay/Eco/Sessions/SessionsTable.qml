pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import Eco.Kit

// SessionsTable lists sessions in the DataTable: the live ones first, under
// LIVE, then the others under the day they were recorded. Each row holds title,
// kind, tags, when, how long, who took part and state — stacked, the tags close
// the second line — toned only while live or importing, a live one's state
// behind a dot; deletable sessions carry a delete button, none while live.
DataTable {
  id: table

  property var sessions: []
  property bool deletable: false
  property var personName: (id, sessionId) => Eco.personName(id, sessionId)

  signal deleteRequested(var session)
  onRemoveRequested: session => { if (deletable) deleteRequested(session) }

  function live(session) { return session.state === "recording" || session.state === "paused" }

  function duration(seconds) {
    const minutes = Math.round(seconds / 60)
    return minutes < 60
      ? I18n.t("history.minutes", { n: minutes })
      : I18n.t("history.hours", { h: Math.floor(minutes / 60), m: String(minutes % 60).padStart(2, "0") })
  }

  // Each session with the group it is listed under.
  rows: sessions.filter(session => live(session)).concat(sessions.filter(session => !live(session)))
    .map(session => Object.assign({ group: live(session) ? I18n.t("live.short") : I18n.day(session.started_at) }, session))
  section: "group"
  columns: [
    { title: I18n.t("session.column.title"), weight: 0.26 },
    { title: I18n.t("session.column.kind"), weight: 0.11 },
    { title: I18n.t("session.column.tags"), weight: 0.14 },
    { title: I18n.t("session.column.recorded"), weight: 0.15 },
    { title: I18n.t("session.column.duration"), weight: 0.09 },
    { title: I18n.t("session.column.people"), weight: 0.15 },
    { title: I18n.t("session.column.status"), weight: 0.10 }
  ]
  cells: session => [
    Eco.titleOf(session),
    Eco.kindName(session.kind),
    (session.tags || []).map(tag => I18n.t("tags.label", { tag: tag })).join(" "),
    I18n.when(session.started_at),
    duration(Eco.ranFor(session.id) ?? session.duration_s),
    (session.people || []).map(id => personName(id, session.id)).join(", "),
    Eco.stateName(session.state)
  ]
  tone: (session, column) => column === 6
    ? ({ recording: Theme.primary, importing: Theme.primary, paused: Theme.warning })[session.state] || Theme.dim
    : ""
  dotted: session => live(session)
  stacked: [[0], [1, 3, 4, 2], [5]]
  status: 6
  emptyText: I18n.t("history.empty")
  toolsWidth: deletable ? 30 : 0
  tools: deletable ? deleteTool : null

  Component {
    id: deleteTool
    IconButton {
      property var entry
      name: "trash"
      tone: Theme.error
      visible: !(entry && Eco.isLive(entry.id))
      tip: I18n.t("delete.action")
      onClicked: table.deleteRequested(entry)
    }
  }
}
