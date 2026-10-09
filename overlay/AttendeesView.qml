import QtQuick
import QtQuick.Layouts

// AttendeesView is who took part in a session, on one line: the people linked
// to it and a dialog to add or remove people.
RowLayout {
  id: attendees

  property string sessionId: ""
  property var peopleIds: []
  property var addedIds: []
  readonly property string names: peopleIds.map(id => Eco.personName(id, sessionId)).join(", ")

  spacing: 10

  Caption { text: I18n.t("attendees.short") }
  Label {
    Layout.fillWidth: true
    text: attendees.names || I18n.t("attendees.none")
    color: attendees.names ? Theme.foreground : Theme.dim
    font.pixelSize: 11
    elide: Text.ElideRight
  }
  IconButton {
    name: "plus"
    tip: I18n.t("attendees.manage")
    onClicked: manage.open()
  }

  AttendeesDialog {
    id: manage
    sessionId: attendees.sessionId
    peopleIds: attendees.peopleIds
    addedIds: attendees.addedIds
  }
}
