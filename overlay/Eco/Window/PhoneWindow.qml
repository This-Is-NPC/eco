import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// PhoneWindow is eco's window on a phone, one portrait screen: whether the
// daemon is reached, and the live level of each input it measures.
Window {
  id: phone

  // What keeps the daemon from running (Eco.phoneStatus), or "".
  property string status: Eco.phoneStatus
  title: "eco"
  width: 360
  height: 720
  visible: true
  color: Theme.background

  ColumnLayout {
    anchors.fill: parent
    anchors.leftMargin: 20 + phone.SafeArea.margins.left
    anchors.topMargin: 20 + phone.SafeArea.margins.top
    anchors.rightMargin: 20 + phone.SafeArea.margins.right
    anchors.bottomMargin: 20 + phone.SafeArea.margins.bottom
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

    StatusMessage {
      objectName: "reason"
      Layout.fillWidth: true
      visible: phone.status !== ""
      kind: phone.status.startsWith("models.preparing") ? "info" : "error"
      message: phone.status === "" ? "" : I18n.t("phone.status." + phone.status)
    }

    TraceButton {
      objectName: "action"
      Layout.fillWidth: true
      visible: phone.status.startsWith("microphone.")
      role: "primary"
      text: I18n.t(phone.status === "microphone.blocked" ? "phone.open_settings" : "phone.allow_microphone")
      onClicked: Eco.phoneAct()
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
