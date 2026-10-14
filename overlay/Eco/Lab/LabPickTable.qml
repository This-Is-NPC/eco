import QtQuick
import Eco.Core
import Eco.Kit

// A DataTable that picks: a click picks a row, a double click opens it.
Column {
  id: sample

  property string selectedId: "first"
  property string said: ""
  property var switches: ({ first: true })
  property string choice: "first"
  property string valueId: "first"

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
  // Key and value: each row's value is its control, which Enter, Space or a
  // double click works.
  DataTable {
    width: parent.width
    height: implicitHeight
    picks: true
    columns: [{ title: I18n.t("interface.column_setting"), weight: 1 }]
    rows: [
      { id: "first", name: I18n.t("lab.option_first") },
      { id: "second", name: I18n.t("lab.option_second") },
      { id: "third", name: I18n.t("lab.option_third"), choice: true }
    ]
    cells: entry => [entry.name]
    selectedId: sample.valueId
    toolsTitle: I18n.t("interface.column_value")
    toolsWidth: 160
    toolsAlways: true
    tools: Item {
      id: value
      property var entry: ({})
      readonly property bool on: sample.switches[value.entry.id] === true
      function act() {
        if (value.entry.choice)
          choice.open()
        else
          sample.switches = Object.assign({}, sample.switches, { [value.entry.id]: !value.on })
      }
      implicitWidth: 166
      implicitHeight: 28
      Chip {
        visible: !value.entry.choice
        anchors.verticalCenter: parent.verticalCenter
        text: I18n.t(value.on ? "settings.fact_on" : "settings.fact_off").toUpperCase()
        checked: value.on
        dim: !value.on
        onClicked: value.act()
      }
      Dropdown {
        id: choice
        visible: !!value.entry.choice
        anchors.verticalCenter: parent.verticalCenter
        options: ["first", "second"]
        current: sample.choice
        describe: option => option === "first" ? I18n.t("lab.option_first") : I18n.t("lab.option_second")
        onPicked: option => sample.choice = option
      }
    }
    onPicked: entry => sample.valueId = entry.id
  }
}
