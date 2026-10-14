import QtQuick
import Eco.Core

// ModelsSection registers the models, chat and transcription: each row its
// name, its type, the provider's id and what uses it; a row opens ModelView.
// The assistant and the default transcription are not deleted; deleting
// another first says what falls back.
SettingsSection {
  id: section

  name: I18n.t("settings.models")
  note: I18n.t("settings.models_note")
  columns: [{ title: I18n.t("settings.column_name"), weight: 0.22 }, { title: I18n.t("settings.column_type"), weight: 0.16 },
    { title: I18n.t("settings.column_model_id"), weight: 0.3 }, { title: I18n.t("settings.column_used_by"), weight: 0.32 }]
  stacked: [[0], [1, 2], [3]]
  add: I18n.t("settings.add_model")
  file: "ModelView.qml"
  rows: config => config.models.map(model => {
    const kept = config.llm.model === model.name || config.stt.model === model.name
    return { id: model.name, removable: !kept, keptReason: kept ? I18n.t("settings.delete_in_use") : "",
      cells: [model.name, typeName(model.type), model.model, links(config, model.name).map(link => link.text).join(", ")] }
  })
  remove: (config, row) => {
    const change = Draft.removal(config, config.models.findIndex(model => model.name === row.id))
    const groups = {}
    change.moves.forEach(move => {
      const label = move.action ? move.action : move.kind ? kindLabel(move.kind, move.use) : I18n.t("settings.translation")
      ;(groups[move.to] = groups[move.to] || []).push(label)
    })
    return {
      title: I18n.t("settings.remove_model_title"),
      question: I18n.t("settings.remove_model_question", { name: row.id }),
      warning: Object.keys(groups).map(to => I18n.t(to ? "settings.remove_falls_back" : "settings.remove_no_model", { users: groups[to].join(", "), model: to }))
        .concat(change.reviewerOff ? [I18n.t("settings.remove_reviewer_off")] : []).join("\n"),
      change: d => Draft.removeModel(d, d.models.findIndex(model => model.name === row.id))
    }
  }

  function typeName(type) {
    return I18n.t(type === "chat" ? "settings.type_chat" : "settings.type_transcription")
  }
  // A session kind's use, as read: the kind, or its translation.
  function kindLabel(kind, use) {
    const name = Eco.kindName(kind).toUpperCase()
    return use === "translation" ? I18n.t("settings.translates_kind", { kind: name }) : name
  }
  // What uses the model `name` in `config`, as links to where each use is set
  // (see Draft.users), each with the title of its section's group.
  function links(config, name) {
    const at = settings.at
    return Draft.users(config, name).map(use => {
      const link = use.role === "assistant" ? settings.link(at.answers, "assistant", I18n.t("settings.role_assistant"))
        : use.role === "reviewer" ? settings.link(at.answers, "reviewer", I18n.t("settings.role_reviewer"))
        : use.role === "transcriber" ? settings.link(at.transcription, "", I18n.t("settings.role_transcriber"))
        : use.role === "translation" ? settings.link(at.translation, "", I18n.t("settings.translation"))
        : use.role === "skill" ? settings.link(at.skills, use.action, use.action)
        : settings.link(at.sessions, use.kind, kindLabel(use.kind, use.use))
      return Object.assign(link, { group: settings.sections[link.section].name })
    })
  }
}
