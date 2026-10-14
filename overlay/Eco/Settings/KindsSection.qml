import QtQuick
import Eco.Core

// KindsSection is the session kinds, the default first: each row the kind,
// whether it is the default, the model its sessions transcribe with and, once
// a chat model is registered, the one they answer with; a row opens KindView.
// MAKE DEFAULT moves the picked kind first at once; the last kind stays, and
// deleting another first says what follows.
SettingsSection {
  // A chat model is registered: kinds answer with one.
  readonly property bool answers: Eco.config !== null && Draft.chats(Eco.config)

  name: I18n.t("settings.kinds")
  note: I18n.t("settings.kind_help")
  columns: [{ title: I18n.t("settings.column_name"), weight: answers ? 0.28 : 0.4 }, { title: I18n.t("settings.kind_default"), weight: answers ? 0.14 : 0.2 },
    { title: I18n.t("settings.column_transcribes"), weight: answers ? 0.29 : 0.4 }].concat(answers ? [{ title: I18n.t("settings.column_answers"), weight: 0.29 }] : [])
  stacked: [[0, 1], [2, 3]]
  add: I18n.t("settings.add_kind")
  file: "KindView.qml"
  rows: config => {
    const chat = Draft.chats(config)
    return config.kinds.map((kind, index) => ({
      id: kind,
      removable: config.kinds.length > 1,
      keptReason: config.kinds.length > 1 ? "" : I18n.t("settings.kind_last"),
      cells: [Eco.kindName(kind), index === 0 ? I18n.t("settings.kind_default") : "", modelName(config, "transcription", kind)]
        .concat(chat ? [modelName(config, "chat", kind)] : [])
    }))
  }
  remove: (config, row) => {
    const change = Draft.kindRemoval(config, row.id)
    return {
      title: I18n.t("settings.remove_kind_title"),
      question: I18n.t("settings.remove_kind_question", { name: Eco.kindName(row.id) }),
      warning: [I18n.t("settings.remove_kind_sessions")]
        .concat(change.heir ? [I18n.t("settings.remove_kind_default", { name: Eco.kindName(change.heir) })] : [])
        .concat(change.own ? [I18n.t("settings.remove_kind_models")] : []).join("\n"),
      change: d => Draft.removeKind(d, row.id)
    }
  }
  commands: (config, row) => [makeDefault(config, row === null ? "" : row.id)]

  // MAKE DEFAULT for the kind `kind` of `config`, saved at once: it moves
  // first; off, saying why, while no kind is picked.
  function makeDefault(config, kind) {
    const at = config.kinds.indexOf(kind)
    return { text: I18n.t("settings.default"), icon: "check",
      tip: I18n.t(at < 0 ? "settings.pick_row_tip" : at === 0 ? "settings.is_default" : "settings.default_tip"), enabled: at > 0,
      run: () => settings.saveNow(d => d.kinds.unshift(d.kinds.splice(d.kinds.indexOf(kind), 1)[0])) }
  }

  // The model sessions of `kind` use for `use` in `config`: their own, or the
  // default they inherit; "" when there is none.
  function modelName(config, use, kind) {
    const answer = Draft.answering(config, use, kind, null)
    return !answer.name || answer.own ? answer.name : I18n.t("settings.default_model", { name: answer.name })
  }
}
