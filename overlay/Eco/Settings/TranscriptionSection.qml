import QtQuick
import Eco.Core

// TranscriptionSection is the default transcription: one item, TranscriptionView.
SettingsSection {
  name: I18n.t("settings.transcription")
  single: true
  file: "TranscriptionView.qml"
  rows: config => [{ id: "transcription" }]
}
