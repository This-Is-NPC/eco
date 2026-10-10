import QtQuick
import Eco.Core
import Eco.Sessions

Column {
  id: sample

  property string selectedId: "planning"
  readonly property var sessions: [
    { id: "planning", title: I18n.t("lab.sample_session"), kind: "meeting", started_at: 1791230400,
      duration_s: 1080, people: [I18n.t("lab.sample_person"), I18n.t("lab.sample_speaker")], tags: [I18n.t("lab.sample_tag")], state: "ended" },
    { id: "english", title: I18n.t("lab.sample_session_second"), kind: "conversation", started_at: 1791144000,
      duration_s: 2940, people: [I18n.t("lab.sample_speaker")], state: "paused" },
    { id: "left", title: I18n.t("lab.sample_session_second"), kind: "conversation", started_at: 1791100800,
      duration_s: 660, people: [I18n.t("lab.sample_person")], state: "interrupted" },
    { id: "ideas", title: I18n.t("lab.sample_session_third"), kind: "idea", started_at: 1791057600,
      duration_s: 420, people: [], tags: [I18n.t("lab.sample_tag"), I18n.t("lab.sample_tag_second")], state: "ended" }
  ]

  spacing: 14

  SessionsTable {
    width: parent.width
    height: implicitHeight
    sessions: sample.sessions
    personName: id => id
    selectedId: sample.selectedId
    onOpened: session => sample.selectedId = session.id
  }

  // What the list says when nothing matches, with its way out.
  SessionsTable {
    width: parent.width
    height: implicitHeight + 40
    emptyText: I18n.t("history.no_filter_match")
    emptyAction: I18n.t("history.clear_filters")
  }
}
