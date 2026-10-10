import QtQuick
import QtQuick.Layouts

// TranscriptionStrip says, a line per source, whose transcription is down and
// reconnecting, and whose is back and how much audio it lost meanwhile. The
// provider's reason shows on hover.
Column {
  id: strip

  // Each source to tell about: {who, color, state ("down" or "back"), code,
  // detail, dropped_s}.
  property var sources: []

  visible: sources.length > 0
  spacing: 4

  Repeater {
    model: strip.sources
    delegate: Item {
      id: row
      required property var modelData
      readonly property bool down: modelData.state === "down"
      readonly property color tone: down ? Theme.warning : Theme.success

      width: strip.width
      implicitHeight: line.implicitHeight

      RowLayout {
        id: line
        anchors { left: parent.left; right: parent.right }
        spacing: 10
        Rectangle { width: 6; height: 6; color: row.modelData.color }
        Label {
          text: row.modelData.who.toUpperCase()
          font.pixelSize: 10
          font.letterSpacing: 2
        }
        Icon { name: row.down ? "warning" : "check"; size: 11; color: row.tone }
        Label {
          Layout.fillWidth: true
          text: row.down
            ? I18n.t(I18n.has("error." + row.modelData.code) ? "error." + row.modelData.code : "error.stt.down")
            : row.modelData.dropped_s >= 0.5
              ? I18n.t("transcription.back_dropped", { n: Math.round(row.modelData.dropped_s) })
              : I18n.t("transcription.back")
          color: row.tone
          font.pixelSize: 10
          font.letterSpacing: 2
          elide: Text.ElideRight
        }
      }

      HoverHandler { id: hover }
      Hint { visible: hover.hovered && row.down && row.modelData.detail !== ""; text: row.modelData.detail }
    }
  }
}
