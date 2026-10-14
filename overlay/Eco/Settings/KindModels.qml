pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core

// KindModels picks the transcription, assistant and translation models of the
// sessions of `kind`, each its own or the default; the assistant and translation
// only once a chat model is registered.
ColumnLayout {
  id: picks

  required property var draft
  required property string kind
  // Applies a change to a copy of the draft.
  signal changed(var change)
  spacing: 12

  Repeater {
    model: [["transcription", I18n.t("settings.type_transcription")]].concat(!Draft.chats(picks.draft) ? [] : [
            ["chat", I18n.t("settings.role_assistant")],
            ["translation", I18n.t("settings.translation")]])
    delegate: ModelPick {
      id: kindPick
      required property var modelData
      Layout.fillWidth: true
      draft: picks.draft
      use: kindPick.modelData[0]
      kind: picks.kind
      text: kindPick.modelData[1]
      // The change holds what it serves, as it may be made again later.
      onPicked: name => {
        const use = kindPick.modelData[0]
        const kind = picks.kind
        picks.changed(d => Draft.serve(d, use, kind, name))
      }
    }
  }
}
