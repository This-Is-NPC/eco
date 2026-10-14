import QtQuick
import Eco.Core

// ModelsSection registers the models, chat and transcription: each row its
// name, its type, the provider's id, what it is the default for and what else
// uses it; a row opens ModelView. The picked model is made a default from the
// bar, saved at once. The chat and transcription defaults are not deleted;
// deleting another first says what falls back.
SettingsSection {
  id: section

  name: I18n.t("settings.models")
  note: I18n.t("settings.models_note")
  columns: [{ title: I18n.t("settings.column_name"), weight: 0.16 }, { title: I18n.t("settings.column_type"), weight: 0.13 },
    { title: I18n.t("settings.column_model_id"), weight: 0.22 }, { title: I18n.t("settings.column_default"), weight: 0.26 },
    { title: I18n.t("settings.column_used_by"), weight: 0.23 }]
  stacked: [[0], [1, 2], [3], [4]]
  add: I18n.t("settings.add_model")
  file: "ModelView.qml"
  rows: config => config.models.map(model => {
    const kept = config.llm.model === model.name || config.stt.model === model.name
    return { id: model.name, removable: !kept, keptReason: kept ? I18n.t("settings.delete_in_use") : "",
      cells: [model.name, typeName(model.type), model.model, defaultsText(config, model.name),
        links(config, model.name).map(link => link.text).join(", ")] }
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

  // DEFAULT FOR CHAT, FOR TRANSLATION and FOR REVIEW for a chat model, FOR
  // TRANSCRIPTION for a transcription one, each off on the model that is
  // already that default; DEFAULT FOR CHAT, off, while no model is picked.
  commands: (config, row) => {
    const model = row === null ? null : config.models.find(model => model.name === row.id)
    if (!model)
      return [{ text: I18n.t("settings.default_chat"), icon: "check", tip: I18n.t("settings.pick_row_tip"), enabled: false, run: () => {} }]
    const is = Draft.defaults(config, model.name)
    const command = (use, text, tip, isTip, short) => {
      const already = is.some(entry => entry.use === use && !entry.inherited)
      return { text: I18n.t(text), short: short ? I18n.t(short) : "", icon: "check", tip: I18n.t(already ? isTip : tip), enabled: !already && !Eco.configSaving,
        run: () => settings.saveNow(d => Draft.makeDefault(d, use, model.name)) }
    }
    return model.type === "chat" ? [
      command("chat", "settings.default_chat", "settings.default_chat_tip", "settings.default_chat_is", "settings.default_chat_short"),
      command("translation", "settings.default_translation", "settings.default_translation_tip", "settings.default_translation_is",
        "settings.default_translation_short"),
      command("review", "settings.default_review", "settings.default_review_tip", "settings.default_review_is", "settings.default_review_short")
    ] : [command("transcription", "settings.default_transcription", "settings.default_transcription_tip", "settings.default_transcription_is")]
  }

  // What the model `name` of `config` is the default for, as read: "" for nothing.
  function defaultsText(config, name) {
    return Draft.defaults(config, name).map(entry => entry.inherited ? I18n.t("settings.translation_inherited")
      : I18n.t(({ chat: "settings.type_chat", transcription: "settings.type_transcription", translation: "settings.translation",
        review: "settings.use_review" })[entry.use])).join(" · ")
  }
  function typeName(type) {
    return I18n.t(type === "chat" ? "settings.type_chat" : "settings.type_transcription")
  }
  // A session kind's use, as read: the kind, or its translation.
  function kindLabel(kind, use) {
    const name = Eco.kindName(kind).toUpperCase()
    return use === "translation" ? I18n.t("settings.translates_kind", { kind: name }) : name
  }
  // What uses the model `name` in `config` besides the defaults, which
  // `defaultsText` says, as links to where each use is set (see Draft.users),
  // each with the title of its group: its section's name, and for a session
  // kind what the kind uses it for.
  function links(config, name) {
    const at = settings.at
    return Draft.users(config, name).filter(use => use.role === "skill" || use.role === "kind").map(use => {
      const link = use.role === "skill" ? settings.link(at.skills, use.action, use.action)
        : settings.link(at.kinds, use.kind, Eco.kindName(use.kind).toUpperCase())
      const uses = use.role === "kind" ? use.uses.filter(use => use !== "transcription")
        .map(use => I18n.t(use === "chat" ? "settings.role_assistant" : "settings.translation")) : []
      return Object.assign(link, { group: [settings.sections[link.section].name].concat(uses.length ? [uses.join(" + ")] : []).join(" · ") })
    })
  }
}
