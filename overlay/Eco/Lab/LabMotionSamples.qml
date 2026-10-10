pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

Column {
  id: motion

  property string view: "first"

  spacing: 14

  Kicker { width: parent.width; text: I18n.t("lab.motion_button") }
  GridLayout {
    width: parent.width
    columns: width < 760 ? 1 : 3
    rowSpacing: 12
    columnSpacing: 12
    Repeater {
      model: ["nav-trace", "delete-trace", "action-trace"]
      delegate: ButtonMotionSample {
        required property string modelData
        variant: modelData
        Layout.fillWidth: true
        Layout.fillHeight: true
        Layout.preferredWidth: (motion.width - (parent.columns - 1) * 12) / parent.columns
      }
    }
  }

  Kicker { width: parent.width; text: I18n.t("lab.screen_transition") }
  Label {
    width: parent.width
    text: I18n.t("lab.screen_transition_help")
    color: Theme.dim
    font.pixelSize: 11
    wrapMode: Text.WordWrap
  }
  Row {
    spacing: 24
    Tab {
      label: I18n.t("lab.view_first")
      digit: "01"
      current: motion.view === "first"
      onChosen: motion.view = "first"
    }
    Tab {
      label: I18n.t("lab.view_second")
      digit: "02"
      current: motion.view === "second"
      onChosen: motion.view = "second"
    }
  }
  Rectangle {
    width: parent.width
    height: 190
    clip: true
    color: Qt.alpha(Theme.primary, 0.025)
    border.color: Theme.line
    LabMotionScreen {
      anchors { fill: parent; margins: 18 }
      shown: motion.view === "first"
      index: "01"
      heading: I18n.t("lab.view_first")
      firstItem: I18n.t("lab.view_first_item_one")
      secondItem: I18n.t("lab.view_first_item_two")
      nextLabel: I18n.t("lab.view_second")
      onNavigate: motion.view = "second"
    }
    LabMotionScreen {
      anchors { fill: parent; margins: 18 }
      shown: motion.view === "second"
      index: "02"
      heading: I18n.t("lab.view_second")
      firstItem: I18n.t("lab.view_second_item_one")
      secondItem: I18n.t("lab.view_second_item_two")
      nextLabel: I18n.t("lab.view_first")
      onNavigate: motion.view = "first"
    }
  }
}
