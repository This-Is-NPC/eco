import QtQuick
import Eco.Core
import Eco.Kit

// A DataTable that picks: a click picks a row, a double click opens it.
Column {
  id: sample

  property string selectedId: "first"
  property string said: ""

  spacing: 8

  Label {
    width: parent.width
    text: I18n.t("lab.picks_help")
    color: Theme.dim
    font.pixelSize: 11
    wrapMode: Text.WordWrap
  }
  DataTable {
    width: parent.width
    height: implicitHeight
    picks: true
    columns: [
      { title: I18n.t("session.column.title"), weight: 0.6 },
      { title: I18n.t("session.column.kind"), weight: 0.4 }
    ]
    rows: [
      { id: "first", title: I18n.t("lab.sample_session"), kind: I18n.t("kind.meeting") },
      { id: "second", title: I18n.t("lab.sample_session_second"), kind: I18n.t("kind.conversation") },
      { id: "third", title: I18n.t("lab.sample_session_third"), kind: I18n.t("kind.idea") }
    ]
    cells: entry => [entry.title, entry.kind]
    stacked: [[0], [1]]
    selectedId: sample.selectedId
    onPicked: entry => {
      sample.selectedId = entry.id
      sample.said = I18n.t("lab.picked", { name: entry.title })
    }
    onOpened: entry => sample.said = I18n.t("lab.opened", { name: entry.title })
  }
  Label {
    text: sample.said
    color: Theme.dim
    font.pixelSize: 11
  }
}
