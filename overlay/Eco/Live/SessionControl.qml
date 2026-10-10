pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SessionControl is the session in one line: its state and running time, the
// live audio running through it like a monitor trace, a warning for an input
// that sends no audio, and the controls. Clicking the state pauses or resumes;
// ending takes a second click. Dense, it fits a masthead row: no trace, no
// state word, icons only, as wide as its content.
Rectangle {
  id: capsule

  property bool confirming: false
  property bool ending: false
  property bool compact: false
  property bool dense: false
  property var activeSession: Eco.session
  property bool linked: Eco.connected
  readonly property color tone: Eco.recording ? Theme.error : Theme.warning
  // Inputs get two seconds after capture restarts before a silence counts.
  property bool settled: false
  // The inputs that send no audio while the session records.
  readonly property var silent: settled && Eco.recording ? Eco.inputs.filter(input => {
    const signal = Eco.signals[input.id]
    return !signal || !signal.alive
  }) : []

  implicitHeight: dense ? 28 : 40
  implicitWidth: line.implicitWidth
  color: Qt.alpha(tone, 0.07)
  border.color: Qt.alpha(tone, 0.55)
  Behavior on color { ColorAnimation { duration: 260 } }
  Behavior on border.color { ColorAnimation { duration: 260 } }

  onActiveSessionChanged: if (activeSession === null) { ending = false; confirming = false }
  onLinkedChanged: if (!linked) { ending = false; confirming = false }

  Timer { id: disarm; interval: 3000; onTriggered: capsule.confirming = false }
  Timer { id: settle; running: true; interval: 2000; onTriggered: capsule.settled = true }

  Connections {
    target: Eco
    function onNoticeCountChanged() {
      if (capsule.ending && Eco.messageIsError)
        capsule.ending = false
    }
    function onRecordingChanged() { capsule.settled = false; settle.restart() }
  }

  RowLayout {
    id: line
    anchors.fill: parent
    spacing: 0

    // State, time and the live trace: one surface, click to pause or resume.
    Item {
      Layout.fillWidth: !capsule.dense
      Layout.fillHeight: true
      implicitWidth: capsule.dense ? readout.x + readout.width + 12 : 0

      Repeater {
        model: capsule.dense ? [] : Eco.inputs
        delegate: EcgTrace {
          required property var modelData
          anchors { fill: parent; leftMargin: readout.x + readout.width + 16; rightMargin: 12; topMargin: 6; bottomMargin: 6 }
          // Only when there is room for a trace worth reading.
          visible: width > 60
          source: Eco.signals[modelData.id] || null
          tone: Eco.inputColor(modelData.id)
          backdrop: Qt.tint(Theme.background, capsule.color)
        }
      }

      Row {
        id: readout
        anchors { left: parent.left; leftMargin: capsule.dense ? 8 : 14; verticalCenter: parent.verticalCenter }
        spacing: capsule.dense ? 8 : 12

        Item {
          anchors.verticalCenter: parent.verticalCenter
          width: 14
          height: 14
          Rectangle {
            anchors.centerIn: parent
            width: 9
            height: 9
            radius: 4.5
            color: capsule.tone
          }
          // A ring that keeps leaving the dot while recording.
          Rectangle {
            id: ring
            anchors.centerIn: parent
            width: 9
            height: 9
            radius: width / 2
            color: "transparent"
            border.color: capsule.tone
            visible: Eco.recording
            ParallelAnimation {
              running: Eco.recording
              loops: Animation.Infinite
              NumberAnimation { target: ring; property: "width"; from: 9; to: 26; duration: 1400; easing.type: Easing.OutCubic }
              NumberAnimation { target: ring; property: "height"; from: 9; to: 26; duration: 1400; easing.type: Easing.OutCubic }
              NumberAnimation { target: ring; property: "opacity"; from: 0.9; to: 0; duration: 1400 }
            }
          }
        }

        Label {
          anchors.verticalCenter: parent.verticalCenter
          text: I18n.elapsed(Eco.session ? Eco.ranFor(Eco.session.id) : 0)
          font.pixelSize: capsule.dense ? 13 : 15
          font.letterSpacing: 1.5
        }
        DecodeLabel {
          anchors.verticalCenter: parent.verticalCenter
          visible: !capsule.compact && !capsule.dense
          value: Eco.recording ? I18n.t("session.recording") : I18n.t("session.paused")
          color: capsule.tone
          font.pixelSize: 9
          font.bold: true
          font.letterSpacing: 3
        }
      }

      MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: Eco.recording ? Eco.pauseSession() : Eco.resumeSession()
      }
    }

    // An input that sends no audio: its name on hover, the audio settings on click.
    IconButton {
      Layout.alignment: Qt.AlignVCenter
      Layout.rightMargin: capsule.dense ? 4 : 8
      Layout.preferredWidth: capsule.dense ? 22 : 28
      Layout.preferredHeight: capsule.dense ? 22 : 28
      visible: capsule.silent.length > 0
      name: "warning"
      tone: Theme.warning
      highlighted: true
      tip: capsule.silent.map(input => input.participant.toUpperCase() + "  ·  " + Eco.deviceLabel(input.id, input.label)).join("\n") + "\n" + I18n.t("inputs.no_signal")
      onClicked: Eco.openConfigAt(0)
    }

    Rectangle { Layout.fillHeight: true; implicitWidth: 1; color: Qt.alpha(capsule.tone, 0.35) }

    CapsuleButton {
      icon: Eco.recording ? "pause" : "play"
      label: Eco.recording ? I18n.t("session.pause") : I18n.t("session.resume")
      tone: capsule.tone
      compact: capsule.compact || capsule.dense
      tip: "SUPER+ALT+P"
      onClicked: Eco.recording ? Eco.pauseSession() : Eco.resumeSession()
    }

    Rectangle { Layout.fillHeight: true; implicitWidth: 1; color: Qt.alpha(capsule.tone, 0.35) }

    CapsuleButton {
      icon: "stop"
      label: capsule.confirming ? I18n.t("session.end_confirm") : I18n.t("session.end")
      tone: Theme.error
      compact: capsule.compact || capsule.dense
      armed: capsule.confirming
      busy: capsule.ending
      onClicked: {
        if (!Eco.connected)
          return
        if (capsule.confirming) {
          capsule.confirming = false
          disarm.stop()
          capsule.ending = true
          Eco.endSession()
        } else {
          capsule.confirming = true
          disarm.restart()
        }
      }
    }
  }
}
