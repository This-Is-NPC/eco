pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SpeakersView is who is who in a session: each speaker label with the person
// it is, eco's guess from the voice waiting to be confirmed or cleared, or the
// name it goes by; CHANGE opens the speaker's assignment. Compact keeps the
// actions' icons only.
ColumnLayout {
  id: view

  property string sessionId: ""
  property bool compact: false
  readonly property var speakers: Eco.speakersOf(sessionId)
  readonly property bool guessing: speakers.some(s => !!s.guess)

  visible: speakers.length > 0
  spacing: 6

  Caption { text: I18n.t("speakers.title") }

  Label {
    Layout.fillWidth: true
    visible: view.guessing
    text: I18n.t("guess.help")
    color: Theme.dim
    font.pixelSize: 10
    wrapMode: Text.WordWrap
  }

  Repeater {
    model: view.speakers
    delegate: RowLayout {
      id: row
      required property var modelData
      readonly property var guess: modelData.guess || null
      readonly property string person: modelData.person || ""
      readonly property string name: modelData.name || modelData.label
      Layout.fillWidth: true
      spacing: 10

      Rectangle {
        implicitWidth: 6
        implicitHeight: 6
        color: Eco.speakerColor(view.sessionId, row.modelData.label, row.name)
      }
      Label {
        Layout.maximumWidth: row.width * 0.4
        text: row.modelData.label + (Eco.isUser(row.modelData.label) ? " " + I18n.t("dialog.you") : "")
        font.pixelSize: 11
        elide: Text.ElideRight
      }
      Label { text: "→"; color: Theme.dim; font.pixelSize: 11 }
      // The person, else the guess, else the name it goes by, else no one.
      Label {
        Layout.fillWidth: true
        text: row.person ? Eco.personName(row.person, view.sessionId)
          : row.guess ? I18n.t("speakers.guess", { name: row.guess.name, score: Math.round(row.guess.score * 100) })
          : row.name !== row.modelData.label ? row.name
          : I18n.t("attendees.none")
        color: row.person ? Eco.personColor(row.person)
          : row.guess ? Theme.primary
          : row.name !== row.modelData.label ? Theme.foreground : Theme.dim
        font.pixelSize: 11
        elide: Text.ElideRight
      }
      Chip {
        visible: row.guess !== null
        icon: "check"
        text: view.compact ? "" : I18n.t("guess.confirm")
        tip: I18n.t("guess.confirm_tip")
        accent: row.guess ? Eco.personColor(row.guess.person) : Theme.primary
        checked: true
        onClicked: Eco.assignPerson(view.sessionId, row.modelData.label, row.guess.person, "", null)
      }
      Chip {
        visible: row.guess !== null || row.person !== ""
        icon: "close"
        text: view.compact ? "" : I18n.t("guess.clear")
        tip: row.person ? I18n.t("speaker.not_them_tip") : I18n.t("guess.clear_tip")
        dim: true
        onClicked: row.person ? Eco.unassignPerson(view.sessionId, row.modelData.label) : Eco.clearGuess(view.sessionId, row.modelData.label)
      }
      Chip {
        icon: "edit"
        text: view.compact ? "" : I18n.t("speakers.change")
        tip: I18n.t("speakers.change_tip")
        dim: true
        onClicked: Eco.speakerRequested({ session: view.sessionId, label: row.modelData.label, name: row.name, scope: "speaker" })
      }
    }
  }
}
