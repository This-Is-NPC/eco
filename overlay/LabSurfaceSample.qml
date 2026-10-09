import QtQuick

SurfaceFrame {
  id: sample

  property string kind: "panel"
  property string heading: I18n.t("lab.sample_label")
  property string bodyText: I18n.t("lab.sample_body")
  property string index: "01"

  implicitHeight: kind === "panel" ? 94 : 112

  Column {
    anchors { fill: parent; margins: 16 }
    spacing: 12
    Row {
      spacing: 10
      Label {
        visible: sample.kind === "panel"
        text: sample.index
        color: Theme.primary
        font.pixelSize: 10
        font.letterSpacing: 2
      }
      Label {
        text: sample.heading.toUpperCase()
        color: sample.variant === "filled" ? Theme.primary : Theme.secondary
        font.pixelSize: 10
        font.letterSpacing: 2
      }
    }
    Label {
      width: parent.width
      text: sample.bodyText
      color: Theme.foreground
      font.pixelSize: 12
      wrapMode: Text.WordWrap
    }
  }
}
