import QtQuick

// SettingsSection describes one section of the settings to SettingsView: its
// table of items, what ADD adds, what DELETE asks and the screen a row opens.
// Each section is a file of its own that sets these.
QtObject {
  // The SettingsView, for its links and saves.
  required property Item settings
  // The tab's name, and what the section is for, above its table.
  property string name
  property string note
  // The table: {title, weight} columns (the weights add up to 1), and the
  // columns on each line of a row stacked on a narrow window.
  property var columns: []
  property var stacked: [[0]]
  // The field of a row naming the title it is listed under, or "" for none: a
  // title opens each run of rows sharing it. A row with `dim` true steps back.
  property string group: ""
  // It needs a chat model: until one is registered it says so above its table.
  property bool chats: false
  // The text of its ADD command, or "" when nothing is added here.
  property string add: ""
  // Its shorter name, for a bar with no room for `add`; "" keeps `add`.
  property string addShort: ""
  // The rows of the saved config `config`: [{id, cells, file, fixed, removable,
  // keptReason, edits, noEdit}]; `file` overrides `file` below, `fixed` marks a
  // part of the config that is never removed (no DELETE), `removable` false
  // keeps DELETE off for now and `keptReason` says why. `edits` false keeps EDIT
  // off on a row whose screen has no fields, and `noEdit` says why.
  property var rows: config => []
  // Each row's own control, beside its cells: a component whose root gets the
  // row as `entry` and whose `act()`, if it has one, Enter, Space and a double
  // click run; how wide it is, and the title over it.
  property Component tools: null
  property real toolsWidth: 0
  property string toolsTitle: ""
  // What shows in place of the table, under the name and the note, or null
  // for the table: created with `settings` (the SettingsView) and `section`
  // (this), it may pick a row through `settings.picked`, whose commands the
  // bar shows, and declare `takeFocus()`, called when the section takes the
  // keyboard.
  property Component body: null
  // The screen a row opens, an ItemPage file beside SettingsView; a new item's
  // row has the id "+".
  property string file: ""
  // The screen ADD opens, when it is not `file`: there the new item is chosen.
  property string adds: ""
  // What deleting `row` from `config` asks and does: {title, question,
  // warning, change}, `change` applied to a copy of the saved config; null
  // when the section deletes nothing.
  property var remove: null
  // The table's own commands, as PageBar takes them, for the row picked or
  // null: each is there either way, off until a row is picked.
  property var commands: (config, row) => []
}
