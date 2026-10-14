pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ChildScreen is a screen inside an item, opened from one of its child tables:
// to add to it, or for a row's own settings. Either it lists what can be added
// (`choose` given), where Enter or its ADD adds the row picked, or it is a small
// form, its children, with its own `commands`. What it does saves at once
// through `finish`, which returns to the child table with the row it names
// picked; BACK and Esc return without saving.
ColumnLayout {
  id: screen

  // The ItemPage it opened in, the name of the child table it returns to, and
  // the child row it opened for (null when it adds); given when it opens.
  property Item page: null
  property string table: ""
  property var row: null
  // Its heading, and what it is for, under it.
  property string title
  property string note
  // To choose from: the table's columns and stacked lines, its rows ({id,
  // cells}), what it says when empty, the text and tip of its main command, and
  // `choose`, row => {change, pick}: the change that adds the row chosen to a
  // copy of the saved config, and the id of the child row then picked.
  property var columns: []
  property var stacked: [[0]]
  property var rows: []
  property string empty
  property string chooseText
  property string chooseTip
  property var choose: null
  // A form's commands, as the bar takes them; BACK comes before them.
  property var commands: []
  default property alias form: fields.data

  property string picked: ""
  readonly property var pickedRow: rows.find(entry => entry.id === picked) || null
  // What the bar shows for it.
  readonly property var actions: choose === null ? commands : [
    { text: chooseText, icon: "plus", role: "primary", tip: chooseTip, enabled: pickedRow !== null && !Eco.configSaving,
      run: () => screen.add(screen.pickedRow) }
  ]

  // Saves `done.change` at once, then returns to the child table, `done.pick` picked.
  function finish(done) { page.store(done.change, table, done.pick) }
  function add(entry) {
    if (entry !== null)
      finish(choose(entry))
  }
  // Gives the keyboard to the rows to choose from, or to the form's first field.
  function takeFocus() {
    if (choose !== null) {
      list.focusRows()
      return
    }
    const first = fields.nextItemInFocusChain(true)
    if (first)
      first.forceActiveFocus()
  }

  Layout.fillWidth: true
  spacing: 14

  ItemNote { visible: text !== ""; text: screen.note }
  DataTable {
    id: list
    Layout.fillWidth: true
    Layout.preferredHeight: implicitHeight
    visible: screen.choose !== null
    picks: true
    columns: screen.columns
    stacked: screen.stacked
    rows: screen.rows
    cells: entry => entry.cells
    selectedId: screen.picked
    emptyText: screen.empty
    onPicked: entry => screen.picked = entry.id
    onOpened: entry => screen.add(entry)
    onBackRequested: screen.page.closeScreen()
  }
  ColumnLayout {
    id: fields
    Layout.fillWidth: true
    spacing: 14
  }
}
