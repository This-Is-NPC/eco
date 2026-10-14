import QtQuick
import Eco.Core

// InterfaceSection is how eco's windows behave: one item, InterfaceView.
SettingsSection {
  name: I18n.t("settings.interface")
  single: true
  file: "InterfaceView.qml"
  rows: config => [{ id: "interface" }]
}
