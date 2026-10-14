pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core

// CommandRow is a bar's actions, ranked: the main one (`role: "primary"`)
// first and solid, then the others outlined, in the order given but the
// guarded ones last, those of one `group` joined as one control whose
// `checked` one is lit. Spread, the main one keeps the left edge and the
// others the right. Each shows its name while the names fit in `room`; when
// they do not, each shows its short name, or, with `iconsWhenTight`, every
// action but the main one shows only its icon, its name in the tip, and when
// even that is wider than `fit`, the main one too; wider still, one menu
// button holds them all, `folds` first, those that cannot run left out.
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
  // The main action at the left edge, the others at the right.
  property bool spread: false

  function isMain(command) { return !command.group && command.role === "primary" }
  readonly property var leading: commands.filter(command => isMain(command))
  readonly property var trailing: commands.filter(command => !isMain(command) && command.role !== "guarded")
    .concat(commands.filter(command => !isMain(command) && command.role === "guarded"))
  readonly property var ranked: leading.concat(trailing)
  readonly property bool tight: measure.implicitWidth > room
  // Tight, and with only the main action named still wider than `fit`.
  readonly property bool cramped: tight && iconsWhenTight && iconsWidth(true) > fit
  // Cramped, and with every action an icon still wider than `fit`.
  readonly property bool folded: cramped && iconsWidth(false) > fit
  // What the menu of a folded row lists.
  readonly property var listed: folds.concat(ranked).filter(command => command.enabled !== false)

  implicitWidth: folded ? menu.implicitWidth : lead.implicitWidth + trail.implicitWidth
    + (leading.length > 0 && trailing.length > 0 ? spacing : 0)
  implicitHeight: Math.max(lead.implicitHeight, trail.implicitHeight, menu.implicitHeight)

  // The row's width with every action that has an icon only that, but the
  // main one when it stays `named`.
  function iconsWidth(named) {
    return commands.reduce((width, command) => width + row.spacing
      + (!command.icon ? 72 : named && isMain(command) ? mainMeasure.implicitWidth : 32), -row.spacing)
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
    text: (row.leading[0] || { text: "" }).text
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

  // Each run of one group together: [[command]].
  function runs(commands) {
    return commands.reduce((all, command) => {
      const last = all.length > 0 ? all[all.length - 1] : null
      if (last && command.group && last[0].group === command.group)
        last.push(command)
      else
        all.push([command])
      return all
    }, [])
  }

  // A run of commands: one alone, or a group joined edge to edge.
  Component {
    id: runDelegate
    Row {
      id: run
      required property var modelData
      spacing: run.modelData.length > 1 ? -1 : 0
      Repeater {
        model: run.modelData
        delegate: TraceButton {
          required property var modelData
          readonly property bool main: row.isMain(modelData)
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

  // The main action, then the others.
  Row {
    id: lead
    visible: !row.folded
    anchors.verticalCenter: parent.verticalCenter
    spacing: row.spacing
    Repeater { model: row.runs(row.leading); delegate: runDelegate }
  }
  Row {
    id: trail
    visible: !row.folded
    anchors.verticalCenter: parent.verticalCenter
    x: row.spread ? row.width - width : lead.width + (row.leading.length > 0 ? row.spacing : 0)
    spacing: row.spacing
    Repeater { model: row.runs(row.trailing); delegate: runDelegate }
  }
}
