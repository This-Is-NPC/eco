import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// PhoneWindow is eco's window on a phone, one portrait screen: whether the
// daemon is reached, and the live level of each input it measures.
Window {
  id: phone

  title: "eco"
  width: 360
  height: 720
  visible: true
  color: Theme.background

  ColumnLayout {
    anchors.fill: parent
    anchors.margins: 20
    spacing: 18

    Masthead {
      objectName: "state"
      Layout.fillWidth: true
      section: I18n.t(Eco.connected ? "phone.connected" : "phone.down")
      Dot {
        objectName: "link"
        filled: Eco.connected
        tone: Eco.connected ? Theme.success : Theme.warning
      }
    }

    Kicker {
      Layout.fillWidth: true
      text: I18n.t("phone.monitor")
    }

    Label {
      Layout.fillWidth: true
      visible: Eco.inputs.length === 0
      text: I18n.t("phone.no_input")
      color: Theme.dim
      font.pixelSize: 10
      font.letterSpacing: 2
      wrapMode: Text.WordWrap
    }

    Repeater {
      model: Eco.inputs
      delegate: ColumnLayout {
        required property var modelData
        Layout.fillWidth: true
        spacing: 4
        Label {
          text: modelData.id.toUpperCase()
          color: Theme.dim
          font.pixelSize: 10
          font.letterSpacing: 2
        }
        EcgTrace {
          objectName: "trace"
          Layout.fillWidth: true
          Layout.preferredHeight: 64
          source: Eco.signals[modelData.id] || null
          tone: Eco.inputColor(modelData.id)
        }
      }
    }

    Item { Layout.fillHeight: true }
  }
}
