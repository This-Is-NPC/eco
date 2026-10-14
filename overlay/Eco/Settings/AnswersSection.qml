import QtQuick
import Eco.Core

// AnswersSection is how answers are made, one row each: the assistant, the
// reviewer that rewrites its answers, the global context sent with every
// answer, then each context slot a session may turn on. Only context slots
// are added and deleted; deleting one first says which kinds lose it.
SettingsSection {
  id: section

  name: I18n.t("settings.answers")
  columns: [{ title: I18n.t("settings.column_name"), weight: 0.26 }, { title: I18n.t("settings.column_model"), weight: 0.24 },
    { title: I18n.t("settings.about"), weight: 0.5 }]
  stacked: [[0, 1], [2]]
  chats: true
  add: I18n.t("contexts.add")
  file: "SlotView.qml"
  rows: config => {
    const kept = { fixed: true }
    const reviewer = config.reviewer || {}
    return [
      Object.assign({ id: "assistant", file: "AssistantView.qml",
        cells: [I18n.t("answers.assistant"), config.llm.model, limits(config.llm)] }, kept),
      Object.assign({ id: "reviewer", file: "ReviewerView.qml",
        cells: [I18n.t("answers.reviewer"), reviewer.model || "",
          reviewer.enabled && reviewer.verbose ? I18n.t("answers.reviewer_verbose") : settings.onOff(!!reviewer.enabled)] }, kept),
      Object.assign({ id: "context", file: "GlobalContextView.qml",
        cells: [I18n.t("answers.context"), "", files(config.context.files) + " · " + I18n.t("answers.always_sent")] }, kept)
    ].concat((config.contexts || []).map(slot => ({ id: slotId(slot.name),
      cells: [slot.name, "", [files(slot.files)].concat(slot.kinds.length ? [I18n.t("answers.slot_kinds", { kinds: kindNames(slot.kinds) })] : []).join(" · ")] })))
  }
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
  // The child table of the files `list` of a context, sent in this order: the
  // global one when `slot` is "", else the slot named so; `screen` adds files.
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
      move: { up: I18n.t("contexts.file_move_up_tip"), down: I18n.t("contexts.file_move_down_tip"),
        change: (d, row, step) => Draft.moveFile(d, slot, row.id, step) }
    }
  }
  function kindName(kind) { return Eco.kindName(kind).toUpperCase() }
  function kindNames(kinds) { return kinds.map(kindName).join(", ") }
  // How much context the assistant takes, and how many answers run at once.
  function limits(llm) {
    return I18n.t("answers.chars", { n: llm.max_context_chars }) + " · " + I18n.t("answers.running", { n: llm.concurrency })
  }
}
