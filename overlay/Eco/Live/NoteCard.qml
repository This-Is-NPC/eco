import QtQuick
import Eco.Core
import Eco.Kit

// NoteCard is a fact the user wrote into a session for later answers to use;
// no one said it. Removed, it also leaves the context of what comes next.
SurfaceFrame {
  id: card

  required property string noteId
  required property string text
  required property string stamp
  // Its removal is a Tab stop; in a list, only while the keyboard is on the card.
  property bool reachable: true

  implicitHeight: heading.height + body.implicitHeight + 34

  Row {
    id: heading
    anchors { left: parent.left; top: parent.top; leftMargin: 14; topMargin: 12 }
    spacing: 10
    Icon { anchors.verticalCenter: parent.verticalCenter; name: "plus"; size: 11; color: Theme.dim }
    Label { text: I18n.t("timeline.note"); color: Theme.dim; font.pixelSize: 10; font.letterSpacing: 3 }
    Label { text: card.stamp; color: Theme.line; font.pixelSize: 9; font.letterSpacing: 1 }
  }

  IconButton {
    anchors { right: parent.right; rightMargin: 14; verticalCenter: heading.verticalCenter }
    name: "close"
    size: 13
    tone: Theme.error
    tip: I18n.t("note.remove_tip")
    activeFocusOnTab: card.reachable
    onClicked: Eco.removeEntry(card.noteId)
  }

  Label {
    id: body
    anchors { left: parent.left; right: parent.right; top: heading.bottom; leftMargin: 14; rightMargin: 14; topMargin: 10 }
    text: card.text
    color: Theme.foreground
    font.pixelSize: 13
    lineHeight: 1.2
    wrapMode: Text.Wrap
  }
}
