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
  // It needs a chat model: until one is registered it says so instead.
  property bool chats: false
  // The text of its ADD command, or "" when nothing is added here.
  property string add: ""
  // The rows of the saved config `config`: [{id, cells, file, fixed, removable,
  // keptReason, open}]; `file` overrides `file` below, `fixed` marks a part of
  // the config that is never removed (no DELETE), `removable` false keeps DELETE
  // off for now and `keptReason` says why. `open`, a command as PageBar takes
  // it, stands for VIEW: its `run`, or else opening the row's screen, is what
  // the command, Enter and a double click do. A row with no screen and no
  // `open.run` has no VIEW.
  property var rows: config => []
  // The screen a row opens, an ItemPage file beside SettingsView; a new item's
  // row has the id "+".
  property string file: ""
  // The screen ADD opens, when it is not `file`: there the new item is chosen.
  property string adds: ""
  // What deleting `row` from `config` asks and does: {title, question,
  // warning, change}, `change` applied to a copy of the saved config; null
  // when the section deletes nothing.
  property var remove: null
  // The table's own commands, as PageBar takes them, for the row picked or null.
  property var commands: (config, row) => []
}
