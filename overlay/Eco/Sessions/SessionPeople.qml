import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SessionPeople is a session's PEOPLE tab: who takes part, and who each
// speaker is, with eco's voice guesses to confirm or clear. It asks for the
// speakers as they are now each time it turns active.
ScrollPage {
  id: page

  property bool active: false
  // The session: {id, people, attendees}.
  property var session: null
  property bool compact: false
  readonly property string sessionId: session ? session.id : ""

  onActiveChanged: if (active) Eco.requestSpeakers(sessionId)
  contentHeight: rows.implicitHeight + 24

  ColumnLayout {
    id: rows
    x: 1
    y: 12
    width: page.width - 2
    spacing: 18

    AttendeesView {
      Layout.fillWidth: true
      sessionId: page.sessionId
      peopleIds: page.session ? page.session.people || [] : []
      addedIds: page.session ? page.session.attendees || [] : []
    }

    SpeakersView {
      Layout.fillWidth: true
      sessionId: page.sessionId
      compact: page.compact
    }
  }
}
