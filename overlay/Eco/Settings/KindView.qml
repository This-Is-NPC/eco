pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// KindView is one session kind. Read, whether it is the default and the models
// its sessions run on, its own or the defaults, with those models and the
// context slots on from the start in it as links; MAKE DEFAULT moves it first
// at once. Edited: its name, never empty or another kind's, which its models
// and context slots follow, and its own transcription, assistant and
// translation models.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property bool fresh: key === "+"
  // Its place in the config being edited: past the end for a new one.
  readonly property int at: item.base === null ? -1 : fresh ? item.base.kinds.length : item.base.kinds.indexOf(key)
  // Its name as typed, or as saved while read.
  readonly property string name: item.active ? item.draft.kinds[at] ?? "" : key
  // The name its models and context slots know it by in the draft (see
  // Draft.renameKind): its saved one until another name is its own.
  property string following: ""
  // The models its sessions transcribe, answer and translate with.
  readonly property var uses: [["transcription", I18n.t("settings.type_transcription")],
    ["chat", I18n.t("settings.role_assistant")], ["translation", I18n.t("settings.translation")]]

  // Renames it to `text` in the draft; the change holds where and from what.
  function rename(text) {
    const at = page.at
    const from = following
    let to = from
    item.update(d => to = Draft.renameKind(d, at, text, from))
    following = to
  }

  kind: I18n.t("settings.item_kind")
  title: name.trim() ? Eco.kindName(name.trim()) : ""
  placeholder: I18n.t("settings.kind_new")
  nextKey: name.trim()
  draft: ItemDraft {
    id: item
    paths: page.at >= 0 ? ["kinds." + page.at] : []
    // Begun or discarded, not rebased: the models know it by its saved name.
    onBegan: page.following = page.fresh ? "" : page.key
  }
  Component.onCompleted: if (fresh) item.begin(d => d.kinds.push(""))

  extra: [section.makeDefault(Eco.config, key)]
  facts: (Eco.config.kinds[0] === key ? [[I18n.t("settings.kind_default"), I18n.t("settings.kind_is_default")]] : [])
    .concat(uses.map(use => [use[1], section.modelName(config, use[0], key) || (use[0] === "transcription" ? "" : I18n.t("settings.no_chat"))]))
  related: {
    const at = settings.at
    const models = [...new Set(uses.map(use => Draft.answering(config, use[0], key, null).name).filter(name => name))]
    return [
      { title: settings.sections[at.models].name, items: models.map(name => settings.link(at.models, name, name)) },
      { title: I18n.t("contexts.slots"), items: (config.contexts || []).filter(slot => slot.kinds.includes(key))
        .map(slot => settings.link(at.context, settings.sections[at.context].slotId(slot.name), slot.name)) }
    ]
  }

  Field {
    Layout.fillWidth: true
    label: I18n.t("settings.kind_name")
    value: page.name
    placeholder: I18n.t("settings.kind_new")
    error: item.errors["kinds." + page.at] || ""
    onEdited: text => page.rename(text)
  }
  KindModels {
    Layout.fillWidth: true
    draft: page.config
    kind: page.following
    onChanged: change => item.update(change)
  }
  ItemNote { text: I18n.t("settings.kind_chat_note") }
}
