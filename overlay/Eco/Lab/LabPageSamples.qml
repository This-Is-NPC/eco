pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import Eco.Kit

// A page's chrome: the tab rail down the left, labelled and folded, its
// second page marked, the page bar beside it with the commands of the page on
// screen, and a command row on its own.
Column {
  id: samples

  property int page: 0
  property string viewMode: "first"
  property string lastAction: ""

  readonly property var pages: [
    { name: I18n.t("lab.option_first") },
    { name: I18n.t("lab.option_second"), marked: true },
    { name: I18n.t("lab.option_third") }
  ]
  readonly property var commands: [
    { text: I18n.t("lab.command.back"), tip: I18n.t("lab.command.back_tip"), role: "back", shown: page > 0, run: () => samples.page = 0 },
    { text: I18n.t("lab.add_entry"), short: I18n.t("lab.command.add_short"), icon: "plus", role: "primary", run: () => samples.lastAction = I18n.t("lab.add_entry") },
    { text: I18n.t("lab.command.view"), short: I18n.t("lab.command.view_short"), icon: "enter", tip: I18n.t("lab.command.view_tip"), run: () => samples.lastAction = I18n.t("lab.command.view") },
    { text: I18n.t("lab.command.delete"), icon: "trash", role: "guarded", run: () => samples.lastAction = I18n.t("lab.command.delete") }
  ]
  // Two outlined actions, then a group whose checked one is lit.
  readonly property var rowCommands: [
    { text: I18n.t("lab.command.view"), short: I18n.t("lab.command.view_short"), icon: "enter", run: () => samples.lastAction = I18n.t("lab.command.view") },
    { text: I18n.t("lab.command.delete"), icon: "trash", role: "guarded", run: () => samples.lastAction = I18n.t("lab.command.delete") },
    { text: I18n.t("lab.option_first").toUpperCase(), group: "mode", checked: viewMode === "first", run: () => samples.viewMode = "first" },
    { text: I18n.t("lab.option_second").toUpperCase(), group: "mode", checked: viewMode === "second", run: () => samples.viewMode = "second" }
  ]

  spacing: 14

  Kicker { width: parent.width; text: I18n.t("lab.tab_rail") + " · " + I18n.t("lab.page_bar") }
  Item {
    width: parent.width
    height: rail.height
    TabRail {
      id: rail
      tabs: samples.pages
      current: samples.page
      onChosen: index => samples.page = index
    }
    PageBar {
      x: rail.width + 16
      width: parent.width - x
      height: 44
      commands: samples.commands
      section: samples.page
    }
  }
  Item {
    width: parent.width
    height: folded.height
    TabRail {
      id: folded
      labelled: false
      tabs: samples.pages
      current: samples.page
      onChosen: index => samples.page = index
    }
    PageBar {
      x: folded.width + 16
      width: Math.min(parent.width - x, 300)
      height: 44
      commands: samples.commands
      section: samples.page
    }
  }

  Label { visible: samples.lastAction !== ""; text: samples.lastAction; color: Theme.dim; font.pixelSize: 10 }

  Kicker { width: parent.width; text: I18n.t("lab.command_row") }
  CommandRow { commands: samples.rowCommands }
}
