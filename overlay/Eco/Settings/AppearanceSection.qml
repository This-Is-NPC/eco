pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import Eco.Kit

// AppearanceSection saves the preset and each custom color independently.
SettingsSection {
  id: section

  readonly property var appearance: Eco.config !== null ? (Eco.config.ui || {}).appearance || { preset: "system", colors: {} } : { preset: "system", colors: {} }
  readonly property bool custom: appearance.preset === "custom"
  readonly property var keys: Object.keys(Theme.dark)
  readonly property var colors: Theme.resolved
  readonly property bool lowContrast: Theme.contrast(Theme.rgb(colors.background), Theme.rgb(colors.foreground)) < 4.5

  name: I18n.t("settings.appearance")
  note: I18n.t(lowContrast ? "appearance.low_contrast" : "appearance.note")
  columns: [{ title: I18n.t("interface.column_setting"), weight: 1 }]
  stacked: [[0]]
  toolsWidth: 230
  toolsTitle: I18n.t("interface.column_value")
  rows: config => [{ id: "preset", fixed: true, cells: [I18n.t("appearance.preset")] }]
    .concat(keys.map(key => ({ id: key, fixed: true, cells: [I18n.t("appearance." + key)], dim: !custom })))
  commands: (config, row) => [{ text: I18n.t("appearance.reset"), enabled: custom && !Eco.configSaving,
    run: () => settings.saveNow(d => d.ui.appearance.colors = Object.assign({}, Theme.dark)) }]

  function choose(preset) {
    const colors = {}
    keys.forEach(key => colors[key] = Theme.resolved[key])
    settings.saveNow(d => {
      d.ui.appearance = d.ui.appearance || { preset: "system", colors: {} }
      if (preset === "custom" && Object.keys(d.ui.appearance.colors).length === 0)
        d.ui.appearance.colors = colors
      d.ui.appearance.preset = preset
    })
  }

  tools: Item {
    id: control
    property var entry: ({})
    readonly property bool preset: entry.id === "preset"
    implicitHeight: preset ? 30 : color.implicitHeight
    implicitWidth: section.toolsWidth
    function act() {
      if (preset)
        presets.open()
      else if (section.custom)
        color.act()
    }
    Dropdown {
      id: presets
      visible: control.preset
      enabled: !Eco.configSaving
      options: ["system", "dark", "light", "custom"]
      current: section.appearance.preset
      describe: preset => I18n.t("appearance." + preset)
      onPicked: preset => section.choose(preset)
    }
    ColorField {
      id: color
      visible: !control.preset
      enabled: section.custom && !Eco.configSaving
      opacity: section.custom ? 1 : 0.45
      width: section.toolsWidth
      value: section.colors[control.entry.id] || Theme.dark.background
      onPicked: value => {
        const key = control.entry.id
        section.settings.saveNow(d => d.ui.appearance.colors[key] = value)
      }
    }
    Connections {
      target: Eco
      function onConfigSaveFailed() { color.input.text = color.value }
    }
  }
}
