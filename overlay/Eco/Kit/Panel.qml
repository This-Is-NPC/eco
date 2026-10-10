import QtQuick
import Eco.Core

// Panel is a framed section with an index, title, tools, and content.
SurfaceFrame {
  id: panel

  property string title
  property string index
  property bool framed: true
  property bool titleSoft: true
  default property alias content: body.data
  property alias tools: toolRow.data

  // Natural height: heading, margins and the first child's own height, so a
  // panel can sit in a stacked layout without being told how tall to be.
  implicitHeight: heading.height + (framed ? 38 : 12) + (body.children.length > 0 ? body.children[0].implicitHeight : 0)
  color: framed ? fill : "transparent"
  border.width: framed ? 1 : 0

  function replayTitle() { headingTitle.replay() }

  Row {
    id: heading
    anchors { left: parent.left; top: parent.top; leftMargin: panel.framed ? 14 : 0; topMargin: panel.framed ? 12 : 0 }
    spacing: 10
    Label {
      visible: panel.index.length > 0
      text: panel.index
      color: panel.tone
      font.pixelSize: 10
      font.letterSpacing: 2
    }
    DecodeLabel {
      id: headingTitle
      value: panel.title.toUpperCase()
      soft: panel.titleSoft
      color: panel.active ? panel.tone : Theme.dim
      font.pixelSize: 10
      font.letterSpacing: 3
    }
  }

  Row {
    id: toolRow
    anchors { right: parent.right; rightMargin: panel.framed ? 14 : 0; verticalCenter: heading.verticalCenter }
    spacing: 8
  }

  Item {
    id: body
    anchors { left: parent.left; right: parent.right; top: heading.bottom; bottom: parent.bottom; margins: panel.framed ? 14 : 0; topMargin: 12 }
    clip: true
  }
}
