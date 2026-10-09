import QtQuick
import "focus.js" as Focus

// ChipStrip keeps chips on one line that scrolls sideways when they run past
// its width — by wheel, drag, or as the keyboard moves along them — and fades
// the edge that hides more.
Item {
  id: strip

  default property alias chips: row.data
  property real spacing: 6
  readonly property Item focused: Window.activeFocusItem
  // Whether the keyboard is on one of its chips.
  readonly property bool holdsFocus: focused !== null && Focus.within(focused, row)

  implicitHeight: row.implicitHeight

  onFocusedChanged: if (holdsFocus) reveal(focused)

  // Scroll so `item`, one of its chips, is in view.
  function reveal(item) {
    const left = item.mapToItem(row, 0, 0).x
    const most = Math.max(0, flick.contentWidth - flick.width)
    if (left < flick.contentX)
      glide.to = Math.max(0, left - 12)
    else if (left + item.width > flick.contentX + flick.width)
      glide.to = Math.min(most, left + item.width - flick.width + 12)
    else
      return
    glide.restart()
  }

  Flickable {
    id: flick
    anchors.fill: parent
    clip: true
    contentWidth: row.implicitWidth
    contentHeight: height
    flickableDirection: Flickable.HorizontalFlick
    boundsBehavior: Flickable.StopAtBounds

    // Under the chips, so the wheel over any of them scrolls the line.
    MouseArea {
      width: Math.max(flick.width, row.implicitWidth)
      height: flick.height
      acceptedButtons: Qt.NoButton
      onWheel: wheel => {
        const most = flick.contentWidth - flick.width
        const step = wheel.pixelDelta.x || wheel.pixelDelta.y || (wheel.angleDelta.x || wheel.angleDelta.y) / 3
        if (most <= 0 || step === 0) {
          wheel.accepted = false
          return
        }
        glide.stop()
        flick.contentX = Math.max(0, Math.min(most, flick.contentX - step))
      }

      Row {
        id: row
        spacing: strip.spacing
      }
    }
  }

  NumberAnimation { id: glide; target: flick; property: "contentX"; duration: 160; easing.type: Easing.OutCubic }

  Rectangle {
    anchors { left: parent.left; top: parent.top; bottom: parent.bottom }
    width: 24
    visible: !flick.atXBeginning
    gradient: Gradient {
      orientation: Gradient.Horizontal
      GradientStop { position: 0; color: Theme.background }
      GradientStop { position: 1; color: Qt.alpha(Theme.background, 0) }
    }
  }
  Rectangle {
    anchors { right: parent.right; top: parent.top; bottom: parent.bottom }
    width: 24
    visible: !flick.atXEnd
    gradient: Gradient {
      orientation: Gradient.Horizontal
      GradientStop { position: 0; color: Qt.alpha(Theme.background, 0) }
      GradientStop { position: 1; color: Theme.background }
    }
  }
}
