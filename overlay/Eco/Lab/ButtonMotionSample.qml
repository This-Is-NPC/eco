import QtQuick
import Eco.Core
import Eco.Kit

Rectangle {
  id: sample

  required property string variant
  property bool selected: false
  property bool armed: false
  property bool busy: false
  property bool completed: false

  readonly property bool destructive: variant === "delete-trace"
  readonly property color tone: destructive ? Theme.error : Theme.primary
  readonly property string state: busy ? "working" : armed ? "armed" : completed ? "done"
    : selected ? (destructive ? "deleted" : "selected") : "ready"
  readonly property string buttonText: armed ? I18n.t("motion.confirm")
    : selected ? I18n.t("motion." + variant + ".selected")
    : completed ? I18n.t("motion.replay") : I18n.t("motion." + variant + ".button")

  implicitHeight: Math.max(160, body.implicitHeight + 28)
  color: Qt.tint(Theme.background, Qt.alpha(Theme.foreground, 0.035))
  border.color: Theme.line

  Column {
    id: body
    anchors { left: parent.left; right: parent.right; top: parent.top; margins: 14 }
    spacing: 9

    Label {
      text: I18n.t("motion." + sample.variant + ".title").toUpperCase()
      color: sample.tone
      font.pixelSize: 11
      font.letterSpacing: 2
    }
    Label {
      width: parent.width
      text: I18n.t("motion." + sample.variant + ".description")
      color: Theme.dim
      font.pixelSize: 11
      wrapMode: Text.WordWrap
    }
    TraceButton {
      width: parent.width
      role: sample.destructive ? "guarded" : sample.variant === "nav-trace" ? "navigation" : "progress"
      icon: sample.destructive ? (sample.selected ? "enter" : "close")
        : sample.variant === "nav-trace" ? "chevron-right" : sample.completed ? "spark" : "play"
      text: sample.buttonText
      armed: sample.armed
      busy: sample.busy
      tone: sample.tone
      onClicked: sample.activate()
    }
    Label {
      text: I18n.t("motion.state." + sample.state).toUpperCase()
      color: sample.state === "ready" ? Theme.dim : sample.tone
      font.pixelSize: 9
      font.letterSpacing: 2
    }
  }

  Timer {
    id: finish
    interval: 1800
    onTriggered: {
      if (sample.destructive) {
        sample.armed = false
        sample.selected = true
      } else
        sample.completed = true
      sample.busy = false
    }
  }

  function activate() {
    if (busy)
      return
    if (variant === "nav-trace") {
      selected = !selected
      return
    }
    if (destructive) {
      if (selected)
        selected = false
      else if (armed) {
        busy = true
        finish.restart()
      } else
        armed = true
    } else {
      completed = false
      busy = true
      finish.restart()
    }
  }
}
