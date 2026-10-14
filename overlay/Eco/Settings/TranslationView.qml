pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// TranslationView is the [translation] table: the model that translates
// sessions whose kind picks none, or, left on the default, the assistant's,
// with a link to the model that does.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property string own: Draft.translation(config).model || ""
  // The model that translates, as the daemon picks it.
  readonly property var answering: Draft.answering(config, "translation", "", null)

  function describe(name) {
    return name || I18n.t("settings.default_model", { name: Draft.answering(page.config, "chat", "", null).name })
  }

  title: section.name.toUpperCase()
  draft: ItemDraft { id: item; paths: ["translation"] }
  facts: [[I18n.t("settings.default_pick"), describe(own)]]
  details: [
    ItemNote { text: I18n.t("settings.translation_note") },
    ItemNote { text: I18n.t("settings.default_translation_tip") }
  ]
  related: [
    { title: settings.sections[settings.at.models].name,
      items: answering.name ? [settings.link(settings.at.models, answering.name, answering.name)] : [] },
    { title: settings.sections[settings.at.answers].name,
      items: answering.own ? [] : [settings.link(settings.at.answers, "assistant", I18n.t("answers.assistant"))] }
  ]

  ItemNote { text: I18n.t("settings.translation_note") }
  PickRow {
    Layout.fillWidth: true
    text: I18n.t("settings.default_pick")
    options: ["", ...Draft.named(page.config, "chat")]
    current: page.own
    describe: name => page.describe(name)
    dim: !page.answering.own
    onPicked: name => item.update(d => {
      d.translation = Draft.translation(d)
      if (name)
        d.translation.model = name
      else
        delete d.translation.model
    })
  }
  ItemNote { text: I18n.t("settings.default_translation_tip") }
}
