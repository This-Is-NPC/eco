pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import Eco.Kit

// PresetView is a secret, provider or model eco ships, read only: what it is,
// and for a model why it is disabled while its key is not set. A user entry
// with the same name replaces it, and then this screen is not shown.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property var preset: section.presetFacts(key)

  kind: I18n.t("presets.item")
  title: key
  facts: preset.facts
  details: [ItemNote { text: I18n.t("presets.note") },
    ItemNote { visible: page.preset.disabled !== ""; text: page.preset.disabled }]
}
