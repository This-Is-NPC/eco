pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core

// CommandRow is a bar's actions, ranked, in the order given: the main one
// (`role: "primary"`) solid, the others outlined, those of one `group` joined
// as one control whose `checked` one is lit. Each shows its name while the
// names fit in `room`; when they do not, each shows its short name, or, with
// `iconsWhenTight`, every action but the main one shows only its icon, its
// name in the tip, and when even that is wider than `fit`, the main one too;
// wider still, one menu button holds them all, `folds` first, those that
// cannot run left out.
Item {
  id: row

  // [{text, short, icon, tip, role, tone, group, checked, enabled, run}];
  // role is "primary", "guarded" or left out.
  property var commands: []
  // How wide the row may be with every name shown.
  property real room: Infinity
  property bool iconsWhenTight: false
  // How wide the row may be at most.
  property real fit: Infinity
  // Commands shown apart from the row that join its menu once it folds, as a
  // bar's BACK.
  property var folds: []
  property real spacing: 10

  // The actions, each run of one group together: [[command]].
  readonly property var runs: commands.reduce((runs, command) => {
      const last = runs.length > 0 ? runs[runs.length - 1] : null
      if (last && command.group && last[0].group === command.group)
        last.push(command)
      else
        runs.push([command])
      return runs
    }, [])
  readonly property bool tight: measure.implicitWidth > room
  // Tight, and with only the main action named still wider than `fit`.
  readonly property bool cramped: tight && iconsWhenTight && iconsWidth(true) > fit
  // Cramped, and with every action an icon still wider than `fit`.
  readonly property bool folded: cramped && iconsWidth(false) > fit
  // What the menu of a folded row lists.
  readonly property var listed: folds.concat(commands).filter(command => command.enabled !== false)

  implicitWidth: folded ? menu.implicitWidth : shown.implicitWidth
  implicitHeight: shown.implicitHeight

  // The row's width with every action that has an icon only that, but the
  // main one when it stays `named`.
  function iconsWidth(named) {
    return commands.reduce((width, command) => width + row.spacing
      + (!command.icon ? 72 : named && !command.group && command.role === "primary" ? mainMeasure.implicitWidth : 32), -row.spacing)
  }

  // The actions with their names, never drawn, to know how wide they are.
  Row {
    id: measure
    // Hidden by opacity: a row that is not visible lays out nothing to measure.
    opacity: 0
    enabled: false
    spacing: row.spacing
    Repeater {
      model: row.commands
      delegate: TraceButton {
        required property var modelData
        dense: true
        icon: modelData.icon || ""
        text: modelData.text
      }
    }
  }

  TraceButton {
    id: mainMeasure
    opacity: 0
    enabled: false
    dense: true
    icon: "enter"
    text: (row.commands.find(command => !command.group && command.role === "primary") || { text: "" }).text
  }

  ActionMenu {
    id: menu
    anchors.verticalCenter: parent.verticalCenter
    visible: row.folded
    tip: I18n.t("detail.more")
    items: row.listed.map((command, index) => ({ id: String(index), icon: command.icon || "", text: command.text,
      guarded: command.role === "guarded", chosen: !!command.checked }))
    onChosen: id => row.listed[Number(id)].run()
  }

  Row {
    id: shown
    visible: !row.folded
    spacing: row.spacing

    Repeater {
      model: row.runs
      // A run of commands: one alone, or a group joined edge to edge.
      delegate: Row {
        id: run
        required property var modelData
        spacing: run.modelData.length > 1 ? -1 : 0
        Repeater {
          model: run.modelData
          delegate: TraceButton {
            required property var modelData
            readonly property bool main: !modelData.group && modelData.role === "primary"
            readonly property bool iconOnly: row.tight && row.iconsWhenTight && (!main || row.cramped) && !!modelData.icon
            dense: true
            role: modelData.group ? (modelData.checked ? "primary" : "action")
              : modelData.role === "primary" || modelData.role === "guarded" ? modelData.role : "action"
            filled: main
            tone: modelData.tone || (role === "guarded" ? Theme.error : modelData.group && !modelData.checked ? Theme.dim : Theme.primary)
            icon: modelData.icon || ""
            text: iconOnly ? "" : row.tight ? modelData.short || modelData.text : modelData.text
            width: iconOnly ? 32 : implicitWidth
            tip: modelData.tip || (iconOnly ? modelData.text.toLowerCase() : "")
            enabled: modelData.enabled !== false
            onClicked: modelData.run()
          }
        }
      }
    }
  }
}
