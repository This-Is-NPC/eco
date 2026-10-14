pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core

// ShortcutsDialog lists every key eco answers to, by where it works; ? or F1
// opens it and the arrows or PgUp/PgDn scroll it.
ModalDialog {
  id: dialog

  maxWidth: 620
  height: Math.min(parent.height - 48, panel.implicitHeight)
  onOpened: scroll.forceActiveFocus()

  // [group, [[keys, what they do]…]]; keys are written as they are pressed.
  readonly property var groups: [
    ["keys.everywhere", [
      ["Tab · Shift+Tab", "keys.move_focus"],
      ["Enter · Space", "keys.press"],
      ["Esc", "keys.back"],
      ["? · F1", "keys.help"]
    ]],
    ["keys.global", [
      ["SUPER+ALT+N", "keys.new_session"],
      ["SUPER+ALT+H", "keys.history"],
      ["SUPER+ALT+C", "keys.settings"],
      ["SUPER+ALT+P", "keys.pause"],
      ["SUPER+ALT+E", "keys.focus_eco"],
      ["SUPER+ALT+A · Q", "keys.actions"]
    ]],
    ["keys.start", [
      ["N", "keys.new_session"],
      ["H", "keys.history"],
      ["I", "keys.import"],
      ["P", "keys.people"]
    ]],
    ["keys.lists", [
      ["↑ · ↓", "keys.choose"],
      ["Enter", "keys.open"],
      ["Delete", "keys.remove"],
      ["N", "keys.new_session"],
      ["I", "keys.import"],
      ["/", "keys.search"]
    ]],
    ["keys.tag_chip", [
      ["F2", "keys.rename_tag"],
      ["Delete", "keys.delete_tag"],
      ["Menu", "keys.tag_menu"]
    ]],
    ["keys.people_screen", [
      ["F2", "keys.rename_person"],
      ["M", "keys.merge_person"]
    ]],
    ["keys.detail", [
      ["R", "keys.resume"],
      ["E", "keys.rename"]
    ]],
    ["keys.timeline", [
      ["↑ · ↓ · PgUp · PgDn", "keys.lines"],
      ["Home · End", "keys.ends"],
      ["N", "keys.name_speaker"],
      ["Tab", "keys.line_tools"],
      ["Enter · Esc", "keys.line_edit"]
    ]],
    ["keys.composer", [
      ["Enter", "keys.send"],
      ["Shift+Enter", "keys.note"],
      ["/", "keys.run_action"],
      ["Alt+1…9", "keys.run_numbered"]
    ]],
    ["keys.menus", [
      ["↑ · ↓ · Home · End", "keys.choose"],
      ["Enter · Space", "keys.pick"],
      ["Esc", "keys.close"]
    ]],
    ["keys.settings_window", [
      ["1…8 · Ctrl+1…8", "keys.tabs"],
      ["Ctrl+Tab · Ctrl+Shift+Tab", "keys.tabs_turn"],
      ["↑ · ↓ · Home · End", "keys.rail_move"],
      ["Enter · Space", "keys.rail_choose"],
      ["↑ · ↓", "keys.pick_row"],
      ["Enter · Space", "keys.open_item"],
      ["Delete", "keys.delete_item"],
      ["Enter", "keys.open_child"],
      ["Delete", "keys.remove_child"],
      ["Ctrl+E", "keys.edit_item"],
      ["Enter · Space", "keys.value_act"],
      ["Ctrl+S", "keys.save"],
      ["Esc", "keys.leave_item"]
    ]]
  ]

  contentItem: DialogPanel {
    id: panel
    dialogOpen: dialog.visible
    implicitHeight: list.implicitHeight + 56
    index: "?"
    title: I18n.t("keys.title")
    active: true

    ScrollPage {
      id: scroll
      anchors.fill: parent
      contentHeight: list.implicitHeight

      ColumnLayout {
        id: list
        width: scroll.width
        spacing: 14
        Repeater {
          model: dialog.groups
          delegate: ColumnLayout {
            id: group
            required property var modelData
            Layout.fillWidth: true
            spacing: 6
            Kicker { Layout.fillWidth: true; text: I18n.t(group.modelData[0]) }
            Repeater {
              model: group.modelData[1]
              delegate: RowLayout {
                id: key
                required property var modelData
                Layout.fillWidth: true
                spacing: 14
                // A long chord wraps within its column rather than running over what it does.
                Label {
                  Layout.preferredWidth: 170
                  Layout.maximumWidth: 170
                  Layout.alignment: Qt.AlignTop
                  wrapMode: Text.Wrap
                  text: key.modelData[0]
                  color: Theme.primary
                  font.pixelSize: 11
                  font.letterSpacing: 1
                }
                Label {
                  Layout.fillWidth: true
                  text: I18n.t(key.modelData[1])
                  color: Theme.dim
                  font.pixelSize: 11
                  wrapMode: Text.Wrap
                }
              }
            }
          }
        }
      }
    }
  }
}
