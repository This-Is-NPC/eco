pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import Eco.Kit

// A page's chrome: the tab rail down the left, its second page marked, the
// page bar beside its head with the commands of the page on screen and its
// readout, and command rows on their own: the guarded command behind a
// hairline, and alone with none.
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
    { text: I18n.t("lab.command.view"), short: I18n.t("lab.command.view_short"), icon: "chevron-right", tip: I18n.t("lab.command.view_tip"), run: () => samples.lastAction = I18n.t("lab.command.view") },
    { text: I18n.t("lab.command.delete"), icon: "trash", role: "guarded", run: () => samples.lastAction = I18n.t("lab.command.delete") }
  ]
  // An outlined action, a group whose checked one is lit, and a guarded one.
  readonly property var rowCommands: [
    { text: I18n.t("lab.command.view"), short: I18n.t("lab.command.view_short"), icon: "chevron-right", run: () => samples.lastAction = I18n.t("lab.command.view") },
    { text: I18n.t("lab.command.delete"), icon: "trash", role: "guarded", run: () => samples.lastAction = I18n.t("lab.command.delete") },
    { text: I18n.t("lab.option_first").toUpperCase(), group: "mode", checked: viewMode === "first", run: () => samples.viewMode = "first" },
    { text: I18n.t("lab.option_second").toUpperCase(), group: "mode", checked: viewMode === "second", run: () => samples.viewMode = "second" }
  ]

  spacing: 14

  Kicker { width: parent.width; text: I18n.t("lab.tab_rail") + " · " + I18n.t("lab.page_bar") }
  Item {
    width: parent.width
    height: rail.height
    PageBar {
      x: rail.foldedWidth + rail.gap
      width: parent.width - x
      height: rail.headHeight
      commands: samples.commands
      section: samples.page
      readout: I18n.t("lab.readout")
    }
    TabRail {
      id: rail
      z: 1
      tabs: samples.pages
      current: samples.page
      onChosen: index => samples.page = index
    }
  }

  Label { visible: samples.lastAction !== ""; text: samples.lastAction; color: Theme.dim; font.pixelSize: 10 }

  Kicker { width: parent.width; text: I18n.t("lab.command_row") }
  CommandRow { commands: samples.rowCommands }
  CommandRow { commands: samples.rowCommands.filter(command => command.role === "guarded") }
}
