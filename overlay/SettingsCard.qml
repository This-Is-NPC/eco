import QtQuick
import QtQuick.Layouts

// SettingsCard is one item of an editable list in the settings: closed, its
// title, a note and, unless something needs it, a removal; open, its fields below.
// A `movable` card also steps up and down its list, by its chevrons or by
// Alt+↑/↓ while the keyboard is in it; `first` and `last` stop it at the ends.
SurfaceFrame {
  id: card

  property string title
  // Shown dim while the title is empty.
  property string placeholder
  property string note
  property bool expanded: false
  property string removeTip
  property bool removable: true
  property bool movable: false
  property bool first: false
  property bool last: false
  default property alias fields: fieldsColumn.data

  signal toggled()
  signal removed()
  // -1 for a step up, 1 for a step down.
  signal moved(int step)

  // Gives the keyboard to the card's open/close switch.
  function takeFocus() { toggle.forceActiveFocus() }

  Layout.fillWidth: true
  implicitHeight: body.implicitHeight + 20
  active: expanded

  Keys.onPressed: event => {
    const step = { [Qt.Key_Up]: -1, [Qt.Key_Down]: 1 }[event.key]
    if (!card.movable || !(event.modifiers & Qt.AltModifier) || step === undefined)
      return
    event.accepted = true
    if (step < 0 ? !card.first : !card.last)
      card.moved(step)
  }

  ColumnLayout {
    id: body
    anchors { left: parent.left; right: parent.right; top: parent.top; margins: 10 }
    spacing: 10

    RowLayout {
      Layout.fillWidth: true
      spacing: 8
      IconButton {
        id: toggle
        name: card.expanded ? "chevron-down" : "chevron-right"
        tip: I18n.t(card.expanded ? "settings.card_close" : "settings.card_open")
        onClicked: card.toggled()
      }
      Label {
        Layout.fillWidth: true
        text: card.title || card.placeholder
        color: card.title ? Theme.foreground : Theme.dim
        font.pixelSize: 12
        elide: Text.ElideRight
        MouseArea {
          anchors.fill: parent
          cursorShape: Qt.PointingHandCursor
          onClicked: card.toggled()
        }
      }
      Label {
        visible: card.note !== ""
        // Titles are short names; the note may take most of the row, cut at its end.
        Layout.maximumWidth: card.width * 0.6
        text: card.note
        color: Theme.dim
        font.pixelSize: 10
        elide: Text.ElideRight
      }
      IconButton {
        visible: card.movable
        enabled: !card.first
        opacity: enabled ? 1 : 0.3
        name: "chevron-up"
        tip: I18n.t("settings.move_up")
        onClicked: card.moved(-1)
      }
      IconButton {
        visible: card.movable
        enabled: !card.last
        opacity: enabled ? 1 : 0.3
        name: "chevron-down"
        tip: I18n.t("settings.move_down")
        onClicked: card.moved(1)
      }
      IconButton {
        visible: card.removable
        name: "close"
        tone: Theme.error
        tip: card.removeTip
        onClicked: card.removed()
      }
    }

    ColumnLayout {
      id: fieldsColumn
      Layout.fillWidth: true
      visible: card.expanded
      spacing: 12
    }
  }
}
