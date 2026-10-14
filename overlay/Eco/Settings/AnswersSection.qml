import QtQuick
import Eco.Core

// AnswersSection is how answers are made, one row each: the assistant, and the
// reviewer that rewrites its answers. Both are always there: nothing is added
// or deleted here.
SettingsSection {
  id: section

  name: I18n.t("settings.answers")
  note: I18n.t("answers.note")
  columns: [{ title: I18n.t("settings.column_name"), weight: 0.26 }, { title: I18n.t("settings.column_model"), weight: 0.24 },
    { title: I18n.t("settings.about"), weight: 0.5 }]
  stacked: [[0, 1], [2]]
  chats: true
  rows: config => {
    const reviewer = config.reviewer || {}
    return [
      { id: "assistant", file: "AssistantView.qml", fixed: true,
        cells: [I18n.t("answers.assistant"), config.llm.model, limits(config.llm)] },
      { id: "reviewer", file: "ReviewerView.qml", fixed: true,
        cells: [I18n.t("answers.reviewer"), reviewer.model || "",
          reviewer.enabled && reviewer.verbose ? I18n.t("answers.reviewer_verbose") : settings.onOff(!!reviewer.enabled)] }
    ]
  }

  // How much context the assistant takes, and how many answers run at once.
  function limits(llm) {
    return I18n.t("answers.chars", { n: llm.max_context_chars }) + " · " + I18n.t("answers.running", { n: llm.concurrency })
  }
}
