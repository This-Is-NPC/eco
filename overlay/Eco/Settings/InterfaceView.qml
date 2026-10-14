pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// InterfaceView is the [ui] table: the interface language, and each switch
// beside what it does.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property var ui: config.ui

  function languageName(code) {
    return code === "auto" ? I18n.t("interface.auto") : (I18n.packs[code] ? I18n.packs[code]._name : code)
  }

  title: section.name.toUpperCase()
  draft: ItemDraft { id: item; paths: ["ui"] }
  facts: [
    [I18n.t("interface.language"), languageName(ui.language)],
    [I18n.t("interface.hide_from_share"), settings.onOff(ui.hide_from_share === true)],
    [I18n.t("interface.tray"), settings.onOff(ui.tray === true)]
  ]

  PickRow {
    text: I18n.t("interface.language")
    options: ["auto", ...I18n.available.map(pack => pack.code)]
    current: page.ui.language
    describe: code => page.languageName(code)
    onPicked: code => item.update(d => d.ui.language = code)
  }

  Repeater {
    model: ["hide_from_share", "tray"]
    RowLayout {
      required property string modelData
      Layout.fillWidth: true
      spacing: 10
      Chip {
        readonly property bool on: page.ui[modelData] === true
        text: I18n.t("interface." + modelData)
        checked: on
        dim: !on
        // The value as clicked, so the change made again sets the same.
        onClicked: {
          const key = modelData
          const value = !on
          item.update(d => d.ui[key] = value)
        }
      }
      Label {
        Layout.fillWidth: true
        text: I18n.t("interface." + modelData + "_tip")
        color: Theme.dim
        font.pixelSize: 10
        wrapMode: Text.Wrap
      }
    }
  }
}
