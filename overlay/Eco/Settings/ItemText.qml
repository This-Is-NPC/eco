import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ItemText is a long text of an item, read: its name over the text as it was
// written, in an outline across the screen. The author's own lines are kept,
// and a line wraps only at the outline's edge.
ColumnLayout {
  id: block

  property string title
  property string text

  Layout.fillWidth: true
  spacing: 6

  Caption { text: block.title.toUpperCase() }
  SurfaceFrame {
    Layout.fillWidth: true
    implicitHeight: body.implicitHeight + 24
    Label {
      id: body
      anchors { left: parent.left; right: parent.right; top: parent.top; topMargin: 12; leftMargin: 14; rightMargin: 14 }
      // Blank lines around it are not part of it.
      text: block.text.replace(/^\s*\n/, "").replace(/\s+$/, "")
      font.pixelSize: 12
      lineHeight: 1.2
      wrapMode: Text.Wrap
    }
  }
}
