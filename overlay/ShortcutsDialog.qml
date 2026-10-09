pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts

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
      ["SUPER+ALT+1 · 2", "keys.actions"]
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
      ["Tab", "keys.line_tools"]
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
      ["Ctrl+1…8", "keys.tabs"],
      ["Ctrl+S", "keys.save"],
      ["Alt+↑ · Alt+↓", "keys.move_card"],
      ["Esc", "keys.close"]
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
                Label {
                  Layout.preferredWidth: 170
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
