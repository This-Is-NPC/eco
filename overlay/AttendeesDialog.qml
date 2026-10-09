pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts

// AttendeesDialog adds people to a session or takes back the ones added by hand.
// The people its speakers are stay apart, as only SPEAKERS changes them. Each
// person wears their colour, and two with one name tell apart by their sessions.
ModalDialog {
  id: dialog

  property string sessionId: ""
  property var peopleIds: []
  property var addedIds: []
  // The people in it through a speaker, and everyone known not in it.
  readonly property var fromSpeakers: peopleIds.filter(id => !addedIds.includes(id))
  readonly property var others: Eco.people.filter(p => !peopleIds.includes(p.id))

  maxWidth: 480
  onOpened: nameBox.forceActiveFocus()

  // A person's name, with how many sessions they are in when someone else
  // known has the same name.
  function nameOf(id) {
    const name = Eco.personName(id, sessionId)
    const person = Eco.people.find(p => p.id === id)
    const namesake = person && Eco.people.some(p => p.id !== id && p.name.toLowerCase() === name.toLowerCase())
    return namesake ? I18n.t("attendees.namesake", { name: name, sessions: I18n.t("people.sessions", { n: person.sessions.length }) }) : name
  }

  function add() {
    Eco.addAttendee(sessionId, "", nameBox.text.trim())
    nameBox.text = ""
  }

  contentItem: DialogPanel {
    dialogOpen: dialog.visible
    implicitHeight: form.implicitHeight + 56
    title: I18n.t("attendees.title")
    active: true

    ColumnLayout {
      id: form
      anchors.fill: parent
      spacing: 14

      ColumnLayout {
        Layout.fillWidth: true
        visible: dialog.fromSpeakers.length > 0
        spacing: 6
        Caption { text: I18n.t("attendees.from_speakers") }
        Flow {
          Layout.fillWidth: true
          spacing: 6
          Repeater {
            model: dialog.fromSpeakers
            // Someone eco no longer keeps leaves every session that names them.
            delegate: Chip {
              required property string modelData
              readonly property bool unknown: Eco.isUnknown(modelData)
              text: unknown ? I18n.t("attendees.removable", { name: dialog.nameOf(modelData) }) : dialog.nameOf(modelData)
              accent: Eco.personColor(modelData)
              swatch: accent
              checked: true
              reason: unknown ? "" : I18n.t("attendees.from_speaker_reason")
              tip: unknown ? I18n.t("attendees.remove_unknown") : ""
              onClicked: Eco.forgetPerson(modelData)
            }
          }
        }
      }

      ColumnLayout {
        Layout.fillWidth: true
        spacing: 6
        Caption { text: I18n.t("attendees.add_someone") }
        RowLayout {
          Layout.fillWidth: true
          TextBox {
            id: nameBox
            Layout.fillWidth: true
            placeholderText: I18n.t("attendees.name")
            onAccepted: if (text.trim().length > 0) dialog.add()
          }
          Chip {
            text: I18n.t("attendees.add")
            enabled: nameBox.text.trim().length > 0
            onClicked: dialog.add()
          }
        }
        Flow {
          Layout.fillWidth: true
          visible: dialog.addedIds.length + dialog.others.length > 0
          spacing: 6
          // The people added by hand, lit, which a click takes back.
          Repeater {
            model: dialog.addedIds.filter(id => dialog.peopleIds.includes(id))
            delegate: Chip {
              required property string modelData
              readonly property bool unknown: Eco.isUnknown(modelData)
              text: I18n.t("attendees.removable", { name: dialog.nameOf(modelData) })
              accent: Eco.personColor(modelData)
              swatch: accent
              checked: true
              tip: unknown ? I18n.t("attendees.remove_unknown") : I18n.t("attendees.remove")
              onClicked: unknown ? Eco.forgetPerson(modelData) : Eco.removeAttendee(dialog.sessionId, modelData)
            }
          }
          // Everyone else known, which a click adds.
          Repeater {
            model: dialog.others
            delegate: Chip {
              required property var modelData
              text: dialog.nameOf(modelData.id)
              accent: Eco.personColor(modelData.id)
              swatch: accent
              dim: true
              onClicked: Eco.addAttendee(dialog.sessionId, modelData.id, "")
            }
          }
        }
      }

      DialogFooter {
        cancelText: I18n.t("settings.close")
        onCancelled: dialog.close()
      }
    }
  }
}
