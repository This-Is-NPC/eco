pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core

// ColorPicker chooses a colour from the input palette, or "auto" (empty) to
// let eco pick by position. The swatch shows the colour in use; the chosen
// one in the grid wears a ring. Enter or Space opens the grid, the arrows move
// in it — past the last row to AUTO — and Enter or Space picks.
Item {
  id: picker

  // The colour set by the user, or "" for automatic.
  property string chosen
  // What automatic resolves to now, shown when nothing is chosen.
  property color automatic: Theme.dim
  signal picked(string color)

  implicitWidth: 22
  implicitHeight: 22
  activeFocusOnTab: true
  Keys.onReturnPressed: grid.open()
  Keys.onSpacePressed: grid.open()

  // The cell the keyboard is on: a palette index, or the palette's length for AUTO.
  property int current: 0
  readonly property int columns: 5

  // Show the grid, as a click on the swatch does.
  function open() { grid.open() }

  function choose(index) {
    grid.close()
    picker.picked(index < Theme.inputPalette.length ? Theme.inputPalette[index] : "")
  }

  Rectangle {
    anchors.fill: parent
    color: "transparent"
    border.color: mouse.containsMouse || grid.opened ? Theme.foreground : Theme.line
    FocusRing { shown: picker.activeFocus }
    Behavior on border.color { ColorAnimation { duration: 120 } }
    Rectangle {
      anchors.fill: parent
      anchors.margins: 4
      color: picker.chosen || picker.automatic
      opacity: picker.chosen ? 1 : 0.5
    }
  }
  MouseArea {
    id: mouse
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: grid.opened ? grid.close() : grid.open()
  }
  Hint { visible: (mouse.containsMouse || picker.activeFocus) && !grid.opened; text: picker.chosen ? I18n.t("color.tip", { color: picker.chosen }) : I18n.t("color.tip_auto") }

  PopupFrame {
    id: grid
    padding: 8
    onAboutToShow: {
      const at = Theme.inputPalette.indexOf(picker.chosen.toLowerCase())
      picker.current = at < 0 ? Theme.inputPalette.length : at
    }

    Column {
      spacing: 8
      focus: true
      Keys.onPressed: event => {
        const last = Theme.inputPalette.length
        const step = { [Qt.Key_Right]: 1, [Qt.Key_Left]: -1, [Qt.Key_Down]: picker.columns, [Qt.Key_Up]: -picker.columns }[event.key]
        if (step !== undefined) {
          // From AUTO, Up goes to the last row's first colour and Left to the last colour.
          const fromAuto = event.key === Qt.Key_Up ? Math.floor((last - 1) / picker.columns) * picker.columns : last + Math.min(step, 0)
          picker.current = Math.max(0, Math.min(last, picker.current === last ? fromAuto : picker.current + step))
        } else if ([Qt.Key_Return, Qt.Key_Enter, Qt.Key_Space].includes(event.key))
          picker.choose(picker.current)
        else
          return
        event.accepted = true
      }
      Grid {
        columns: picker.columns
        spacing: 6
        Repeater {
          model: Theme.inputPalette
          delegate: Rectangle {
            id: swatch
            required property string modelData
            required property int index
            readonly property bool current: picker.chosen.toLowerCase() === modelData
            width: 22
            height: 22
            color: "transparent"
            border.color: current ? Theme.foreground : (pick.containsMouse ? Theme.dim : "transparent")
            Rectangle { anchors.fill: parent; anchors.margins: 3; color: swatch.modelData }
            FocusRing { shown: picker.current === swatch.index; anchors.margins: -2 }
            MouseArea {
              id: pick
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: picker.choose(swatch.index)
            }
          }
        }
      }
      Chip {
        width: parent.width
        text: I18n.t("color.auto")
        checked: picker.chosen === ""
        activeFocusOnTab: false
        onClicked: picker.choose(Theme.inputPalette.length)
        FocusRing { shown: picker.current === Theme.inputPalette.length }
      }
    }
  }
}
