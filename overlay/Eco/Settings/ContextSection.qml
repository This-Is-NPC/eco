import QtQuick
import Eco.Core

// ContextSection is the files sent with answers, one row each: the global
// context, sent with every answer and never deleted, then each context slot a
// session may turn on. Only slots are added and deleted; deleting one first
// says which kinds lose it.
SettingsSection {
  id: section

  name: I18n.t("settings.context")
  note: I18n.t("context.note")
  columns: [{ title: I18n.t("settings.column_name"), weight: 0.3 }, { title: I18n.t("context.column_sent"), weight: 0.45 },
    { title: I18n.t("contexts.files"), weight: 0.25 }]
  stacked: [[0, 2], [1]]
  chats: true
  add: I18n.t("contexts.add")
  addShort: I18n.t("contexts.add_short")
  file: "SlotView.qml"
  rows: config => [
    { id: "context", file: "GlobalContextView.qml", fixed: true, edits: false, noEdit: I18n.t("context.global_no_edit"),
      cells: [I18n.t("context.global"), I18n.t("context.every_session"), files(config.context.files)] }
  ].concat((config.contexts || []).map(slot => ({ id: slotId(slot.name),
    cells: [slot.name, slot.kinds.length ? I18n.t("answers.slot_kinds", { kinds: kindNames(slot.kinds) }) : I18n.t("context.when_on"), files(slot.files)] })))
  remove: (config, row) => {
    const name = slotName(row.id)
    const slot = config.contexts.find(slot => slot.name === name)
    return {
      title: I18n.t("answers.remove_slot_title"),
      question: I18n.t("answers.remove_slot_question", { name: name }),
      warning: slot.kinds.length ? I18n.t("answers.remove_slot_kinds", { kinds: kindNames(slot.kinds) }) : "",
      change: d => d.contexts = d.contexts.filter(slot => slot.name !== name)
    }
  }

  // A context slot's row id, and the slot's name from its row id.
  function slotId(name) { return "slot:" + name }
  function slotName(id) { return id.slice("slot:".length) }
  function files(list) { return I18n.t("contexts.files_count", { n: list.length }) }
  // The child table of the files `list` of a context, sent in the order they
  // were added: the global one when `slot` is "", else the slot named so;
  // `screen` adds files.
  function filesTable(list, slot, screen) {
    return {
      name: "files",
      title: I18n.t("contexts.files"),
      empty: I18n.t("contexts.files_none"),
      columns: [{ title: I18n.t("contexts.column_path"), weight: 1 }],
      stacked: [[0]],
      rows: list.map(path => ({ id: path, cells: [path] })),
      add: { text: I18n.t("contexts.add_file"), tip: I18n.t("contexts.add_file_tip"), screen: screen },
      remove: row => ({ question: "", change: d => Draft.removeFile(d, slot, row.id) }),
      removeText: I18n.t("contexts.remove_file"),
      pickTip: I18n.t("contexts.pick_file_tip")
    }
  }
  function kindName(kind) { return Eco.kindName(kind).toUpperCase() }
  function kindNames(kinds) { return kinds.map(kindName).join(", ") }
}
