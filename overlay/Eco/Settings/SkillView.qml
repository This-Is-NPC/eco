pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SkillView is one skill. Read, the model it answers with, its hook and
// whether the hook runs on its own, then its prompt and output format, with
// its model a link. Edited: its name, prompt, output format, model and hook: a
// command run with the session it answered for, on its own or when sent.
// Shortcuts call a skill by its name, so a new name says so.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property bool fresh: key === "+"
  // Its place in the config being edited: past the end for a new one.
  readonly property int at: item.base === null ? -1 : fresh ? item.base.actions.length : item.base.actions.findIndex(action => action.name === key)
  readonly property var entry: (item.active ? item.draft.actions[at] : config.actions.find(action => action.name === key))
    || ({ name: "", prompt: "", format: "" })
  readonly property var answer: Draft.answering(config, "chat", "", entry)

  // Sets its `name` to `value`; the change holds where, as it may be made again later.
  function change(name, value) {
    const at = page.at
    item.update(d => {
      const action = d.actions[at]
      if ((name === "model" || name === "hook" || name === "hook_auto") && !value)
        delete action[name]
      else
        action[name] = value
      // Without a hook there is nothing to send on its own.
      if (!action.hook)
        delete action.hook_auto
    })
  }

  kind: I18n.t("settings.item_skill")
  title: entry.name
  placeholder: I18n.t("settings.skill_new")
  nextKey: entry.name.trim()
  draft: ItemDraft {
    id: item
    paths: page.at >= 0 ? ["actions." + page.at] : []
  }
  Component.onCompleted: if (fresh) item.begin(d => d.actions.push({ name: "", prompt: "", format: "" }))

  facts: [
    [I18n.t("settings.action_model"), section.modelName(config, entry)],
    [I18n.t("settings.action_hook"), entry.hook || ""]
  ].concat(entry.hook ? [[I18n.t("settings.action_hook_auto"), settings.onOff(!!entry.hook_auto)]] : [])
  related: answer.name ? [{ title: settings.sections[settings.at.models].name, items: [settings.link(settings.at.models, answer.name, answer.name)] }] : []
  // Its prompt and output format, each under its name.
  details: Repeater {
    model: [[I18n.t("settings.action_prompt"), page.entry.prompt], [I18n.t("settings.action_format"), page.entry.format]].filter(text => text[1] !== "")
    ItemText {
      required property var modelData
      title: modelData[0]
      text: modelData[1]
    }
  }

  Field {
    Layout.fillWidth: true
    label: I18n.t("settings.action_name")
    value: page.entry.name
    placeholder: I18n.t("settings.skill_new")
    error: item.errors["actions." + page.at] || ""
    onEdited: text => page.change("name", text)
  }
  Label {
    Layout.fillWidth: true
    visible: !page.fresh && page.entry.name.trim() !== page.key
    text: I18n.t("settings.action_renamed")
    color: Theme.warning
    font.pixelSize: 11
    wrapMode: Text.Wrap
  }
  SettingsTextArea { Layout.fillWidth: true; label: I18n.t("settings.action_prompt"); value: page.entry.prompt; onEdited: text => page.change("prompt", text) }
  SettingsTextArea { Layout.fillWidth: true; label: I18n.t("settings.action_format"); value: page.entry.format; onEdited: text => page.change("format", text) }
  ModelPick {
    Layout.fillWidth: true
    draft: page.config
    skill: page.at
    text: I18n.t("settings.action_model")
    onPicked: name => page.change("model", name)
  }
  Field { Layout.fillWidth: true; label: I18n.t("settings.action_hook"); value: page.entry.hook || ""; onEdited: text => page.change("hook", text) }
  ItemNote { text: I18n.t("settings.action_hook_note") }
  Chip {
    enabled: !!page.entry.hook
    text: I18n.t("settings.action_hook_auto")
    tip: I18n.t("settings.action_hook_auto_tip")
    checked: !!page.entry.hook_auto
    onClicked: page.change("hook_auto", !page.entry.hook_auto)
  }
}
