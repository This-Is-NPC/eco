pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ChooseView is a section's screen that chooses one row of a table: Enter, a
// double click or its command goes back to the section's table, then saves
// the choice at once.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  // What it is for, under the title; the table's columns, stacked lines, rows
  // ({id, cells}) and what it says when empty.
  property string note
  property var columns: []
  property var stacked: [[0]]
  property var rows: []
  property string empty
  // Its command.
  property string chooseText
  property string chooseIcon
  property string chooseTip
  // row => {change, pick}: the change that saves the row chosen to a copy of
  // the saved config, or null when it changes nothing, and the id of the
  // section's row picked once it is saved, or undefined to keep the one picked.
  property var choose: null
  property string picked: ""
  readonly property var pickedRow: rows.find(entry => entry.id === picked) || null

  // Goes back first, so the table is gone before the save changes its rows,
  // then saves `entry`.
  function chosen(entry) {
    if (entry === null || Eco.configSaving)
      return
    const done = choose(entry)
    const view = settings
    const index = view.sections.indexOf(section)
    back()
    if (done.change)
      view.saveNow(done.change, done.pick === undefined ? null : () => view.picked = view.setOf(view.picked, index, done.pick))
  }
  function takeFocus() { list.focusRows() }

  kind: section.name
  extra: [
    { text: page.chooseText, icon: page.chooseIcon, role: "primary", tip: page.chooseTip,
      enabled: page.pickedRow !== null && !Eco.configSaving, run: () => page.chosen(page.pickedRow) }
  ]
  details: [
    ItemNote { Layout.fillWidth: true; text: page.note },
    DataTable {
      id: list
      Layout.fillWidth: true
      Layout.preferredHeight: implicitHeight
      picks: true
      columns: page.columns
      stacked: page.stacked
      rows: page.rows
      cells: entry => entry.cells
      selectedId: page.picked
      emptyText: page.empty
      onPicked: entry => page.picked = entry.id
      onOpened: entry => page.chosen(entry)
      onBackRequested: page.leave()
    }
  ]
}
