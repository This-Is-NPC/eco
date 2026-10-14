import QtQuick
import Eco.Core

// SkillsSection is the skills, in the order the composer offers them, the
// order they were added: each row its name, the model it answers with and its
// hook; a row opens SkillView. Deleting one says that shortcuts calling it
// stop and the ones after it move up.
SettingsSection {
  name: I18n.t("settings.skills")
  columns: [{ title: I18n.t("settings.column_name"), weight: 0.3 }, { title: I18n.t("settings.action_model"), weight: 0.4 },
    { title: I18n.t("settings.action_hook"), weight: 0.3 }]
  stacked: [[0], [1, 2]]
  chats: true
  add: I18n.t("settings.add_action")
  file: "SkillView.qml"
  rows: config => config.actions.map(action => ({ id: action.name, cells: [action.name, modelName(config, action), hookState(action)] }))
  remove: (config, row) => {
    const last = config.actions[config.actions.length - 1].name === row.id
    return {
      title: I18n.t("settings.remove_action_title"),
      question: I18n.t("settings.remove_action_question", { name: row.id }),
      warning: [I18n.t("settings.remove_action_shortcut", { name: row.id })].concat(last ? [] : [I18n.t("settings.remove_action_order")]).join("\n"),
      change: d => d.actions.splice(d.actions.findIndex(action => action.name === row.id), 1)
    }
  }

  // The model the skill `action` answers with in `config`: its own, or the
  // default it inherits.
  function modelName(config, action) {
    const answer = Draft.answering(config, "chat", "", action)
    return answer.own ? answer.name : I18n.t("settings.default_model", { name: answer.name })
  }
  // Whether the skill has a hook, and whether it sends on its own.
  function hookState(action) {
    return action.hook ? I18n.t(action.hook_auto ? "settings.action_hook_on" : "settings.action_hook_send") : ""
  }
}
