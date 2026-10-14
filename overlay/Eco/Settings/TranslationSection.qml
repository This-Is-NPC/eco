import QtQuick
import Eco.Core

// TranslationSection is the default translation model: one item, TranslationView.
SettingsSection {
  name: I18n.t("settings.translation_panel")
  single: true
  chats: true
  file: "TranslationView.qml"
  rows: config => [{ id: "translation" }]
}
