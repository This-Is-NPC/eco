import QtQuick
import QtQuick.Layouts
import Eco.Core

// CapsuleButton is a segment of the session capsule: an icon beside a small
// label, lit on hover, filled when armed; Enter or Space presses it.
Item {
  id: button

  property string icon
  property string label
  property color tone: Theme.primary
  property bool armed: false
  property bool busy: false
  // Compact keeps the icon and drops the label.
  property bool compact: false
  property string tip
  signal clicked()

  activeFocusOnTab: true
  function press() { if (!button.busy) button.clicked() }
  Keys.onReturnPressed: press()
  Keys.onSpacePressed: press()

  onArmedChanged: if (armed) guardNudge.restart()

  Layout.fillHeight: true
  Layout.preferredWidth: compact ? 44 : Math.max(86, content.implicitWidth + 28)
  Behavior on Layout.preferredWidth { NumberAnimation { duration: 200; easing.type: Easing.OutCubic } }

  Rectangle {
    id: surface
    anchors.fill: parent
    anchors.margins: 1
    clip: true
    color: button.armed && !button.busy ? Qt.alpha(button.tone, 0.85) : (mouse.containsMouse ? Qt.alpha(button.tone, 0.12) : "transparent")
    Behavior on color { ColorAnimation { duration: 160 } }

    LoadingTrace {
      id: trace
      anchors { fill: parent; leftMargin: 6; rightMargin: 6; topMargin: 7; bottomMargin: 7 }
      running: button.busy
      tone: button.tone
      backdrop: surface.color
    }

    Rectangle {
      anchors { left: parent.left; bottom: parent.bottom }
      width: mouse.containsMouse && !button.busy ? parent.width : 0
      height: 2
      color: button.armed ? Theme.background : button.tone
      Behavior on width { NumberAnimation { duration: 220; easing.type: Easing.OutCubic } }
    }
  }

  Row {
    id: content
    anchors.centerIn: parent
    spacing: 8
    // Back once the trace has reached the end and gone.
    visible: !trace.visible
    transform: Translate { id: contentShift }
    scale: mouse.pressed ? 0.94 : 1
    Behavior on scale { NumberAnimation { duration: 90 } }
    Icon {
      anchors.verticalCenter: parent.verticalCenter
      name: button.icon
      size: 13
      color: button.armed ? Theme.background : (mouse.containsMouse ? button.tone : Theme.foreground)
    }
    Label {
      id: caption
      visible: !button.compact
      anchors.verticalCenter: parent.verticalCenter
      text: button.label
      color: button.armed ? Theme.background : (mouse.containsMouse ? button.tone : Theme.dim)
      font.pixelSize: 9
      font.letterSpacing: 2.5
    }
  }

  MouseArea {
    id: mouse
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: button.press()
  }

  GuardNudge { id: guardNudge; targetObject: contentShift }

  FocusRing {}

  Hint {
    visible: (mouse.containsMouse || button.activeFocus) && (button.tip.length > 0 || button.compact)
    text: button.compact ? button.label + (button.tip ? "  ·  " + button.tip : "") : button.tip
  }
}
