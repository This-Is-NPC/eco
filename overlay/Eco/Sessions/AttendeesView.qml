pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// AttendeesView is who takes part in a session: the people its speakers are,
// which only SPEAKERS changes, the people added by hand, which a click takes
// back, and everyone else known, which a click adds. Each person wears their
// colour, and two with one name tell apart by their sessions.
ColumnLayout {
  id: attendees

  property string sessionId: ""
  property var peopleIds: []
  property var addedIds: []
  // The people in it through a speaker, and everyone known not in it.
  readonly property var fromSpeakers: peopleIds.filter(id => !addedIds.includes(id))
  readonly property var others: Eco.people.filter(p => !peopleIds.includes(p.id))

  spacing: 6

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

  Caption { text: I18n.t("attendees.short") }

  Flow {
    Layout.fillWidth: true
    visible: attendees.fromSpeakers.length > 0
    spacing: 6
    Repeater {
      model: attendees.fromSpeakers
      // Someone eco no longer keeps leaves every session that names them.
      delegate: Chip {
        required property string modelData
        readonly property bool unknown: Eco.isUnknown(modelData)
        text: unknown ? I18n.t("attendees.removable", { name: attendees.nameOf(modelData) }) : attendees.nameOf(modelData)
        accent: Eco.personColor(modelData)
        swatch: accent
        checked: true
        reason: unknown ? "" : I18n.t("attendees.from_speaker_reason")
        tip: unknown ? I18n.t("attendees.remove_unknown") : ""
        onClicked: Eco.forgetPerson(modelData)
      }
    }
  }

  RowLayout {
    Layout.fillWidth: true
    TextBox {
      id: nameBox
      Layout.fillWidth: true
      placeholderText: I18n.t("attendees.name")
      onAccepted: if (text.trim().length > 0) attendees.add()
    }
    Chip {
      text: I18n.t("attendees.add")
      enabled: nameBox.text.trim().length > 0
      onClicked: attendees.add()
    }
  }

  Flow {
    Layout.fillWidth: true
    visible: attendees.addedIds.length + attendees.others.length > 0
    spacing: 6
    // The people added by hand, lit, which a click takes back.
    Repeater {
      model: attendees.addedIds.filter(id => attendees.peopleIds.includes(id))
      delegate: Chip {
        required property string modelData
        readonly property bool unknown: Eco.isUnknown(modelData)
        text: I18n.t("attendees.removable", { name: attendees.nameOf(modelData) })
        accent: Eco.personColor(modelData)
        swatch: accent
        checked: true
        tip: unknown ? I18n.t("attendees.remove_unknown") : I18n.t("attendees.remove")
        onClicked: unknown ? Eco.forgetPerson(modelData) : Eco.removeAttendee(attendees.sessionId, modelData)
      }
    }
    // Everyone else known, which a click adds.
    Repeater {
      model: attendees.others
      delegate: Chip {
        required property var modelData
        text: attendees.nameOf(modelData.id)
        accent: Eco.personColor(modelData.id)
        swatch: accent
        dim: true
        onClicked: Eco.addAttendee(attendees.sessionId, modelData.id, "")
      }
    }
  }
}
