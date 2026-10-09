import QtQuick
import QtQuick.Layouts

// Masthead is the window's top line: the name, a section, the readout on the
// right, and a hairline that fades out from an accent notch. Tight, it packs
// closer to give its height and width to what is below and on the right.
ColumnLayout {
  id: mast
  property string section
  property bool tight: false
  // Whether the name leads the line; without it, the section does.
  property bool branded: true
  default property alias trailing: tail.data
  spacing: tight ? 6 : 10
  onSectionChanged: sweep.restart()

  RowLayout {
    Layout.fillWidth: true
    spacing: 10
    Label { visible: mast.branded; text: "ECO"; font.pixelSize: 15; font.bold: true; font.letterSpacing: 6 }
    Label { visible: mast.branded; text: "╱"; color: Theme.primary; font.pixelSize: 13 }
    DecodeLabel {
      Layout.fillWidth: true
      // The section name keeps its room before the readout on the right, but
      // elides on a narrow window: a wider minimum would widen the whole view.
      Layout.minimumWidth: Math.min(implicitWidth, mast.tight ? 0 : 60)
      value: mast.section.toUpperCase()
      color: Theme.dim
      font.pixelSize: 10
      font.letterSpacing: 3
      elide: Text.ElideRight
    }
    RowLayout { id: tail; spacing: mast.tight ? 8 : 14 }
  }

  Item {
    Layout.fillWidth: true
    implicitHeight: 1
    Rectangle {
      id: notch
      width: 28
      height: 1
      color: Theme.primary
      // The notch sweeps along the rule whenever the section changes.
      SequentialAnimation {
        id: sweep
        NumberAnimation { target: notch; property: "x"; from: 0; to: mast.width - 28; duration: 420; easing.type: Easing.InOutCubic }
        NumberAnimation { target: notch; property: "x"; to: 0; duration: 0 }
      }
    }
    Rectangle {
      x: 32
      width: parent.width - 32
      height: 1
      gradient: Gradient {
        orientation: Gradient.Horizontal
        GradientStop { position: 0; color: Theme.line }
        GradientStop { position: 1; color: "transparent" }
      }
    }
  }
}
