import QtQuick
import Eco.Core

// PromptsSection is the instructions the models get, one row each: the rules
// every answer follows, and what the reviewer is asked. Each row opens
// PromptView, where its text is read and edited. Both are always there:
// nothing is added or deleted here.
SettingsSection {
  name: I18n.t("settings.prompts")
  note: I18n.t("prompts.note")
  columns: [{ title: I18n.t("settings.column_name"), weight: 0.32 }, { title: I18n.t("prompts.column_text"), weight: 0.68 }]
  stacked: [[0], [1]]
  chats: true
  file: "PromptView.qml"
  rows: config => ["rules", "reviewer"].map(id => ({ id: id, fixed: true, cells: [title(id), firstLine(text(config, id))] }))

  // The prompt `id` names.
  function title(id) { return I18n.t(id === "rules" ? "prompts.rules" : "prompts.reviewer") }
  // What the prompt `id` says in `config`.
  function text(config, id) { return id === "rules" ? config.rules : (config.reviewer || {}).prompt || "" }
  // The draft `d` with the prompt `id` saying `text`.
  function write(d, id, text) {
    if (id === "rules")
      d.rules = text
    else
      d.reviewer = Object.assign({ enabled: false, model: "" }, d.reviewer, { prompt: text })
  }
  // The config path the prompt `id` is kept at.
  function path(id) { return id === "rules" ? "rules" : "reviewer.prompt" }
  // The first line of `text` with something on it, for its row.
  function firstLine(text) { return text.split("\n").map(line => line.trim()).find(line => line !== "") || "" }
}
