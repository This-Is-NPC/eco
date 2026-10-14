pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import Eco.Kit

// ConfigWindow is the settings: the sections down the left, beside them the
// bar of the commands of the one on screen, and under the bar the section
// itself (SettingsView), where each item is saved on its own; the daemon
// validates each save, writes config.toml and restarts capture.
Window {
  id: window

  // The sections, for what drives the window.
  readonly property SettingsView view: settings
  // Some item has changes not saved.
  readonly property bool dirty: settings.dirty

  // Fixed, untranslated: packaging/hypr/eco.lua matches the window by this title.
  title: "eco · settings"
  // Fits the screen it opens on, with a margin.
  width: Math.min(1040, screen ? screen.width - 96 : 1040)
  height: Math.min(760, screen ? screen.height - 96 : 760)
  minimumWidth: 240
  minimumHeight: 320
  color: Theme.background
  // Shown while the settings are open. The window is created once and then kept:
  // destroying a window makes Qt write its pipeline cache to disk, and the whole
  // interface waits for that, often for seconds.
  visible: Eco.configOpen
  // Closed by the compositor: the window is kept, as above, and the settings
  // close, unless an item has changes not saved: then it asks first.
  onClosing: close => {
    close.accepted = false
    window.attemptClose()
  }

  property int page: 0

  // Chooses a section and gives it the keyboard, unless the keyboard is to
  // stay where it is, as on the rail.
  function choose(index, stay) {
    settings.forget()
    page = (index + settings.sections.length) % settings.sections.length
    Qt.callLater(stay ? () => rail.forceActiveFocus() : settings.takeFocus)
  }
  function attemptClose() {
    if (dirty)
      confirmClose.open()
    else
      Eco.configOpen = false
  }

  Connections {
    target: Eco
    function onConfigCloseRequested() { window.attemptClose() }
    function onConfigPageRequested(page) { window.choose(page) }
  }

  Shortcut { sequences: ["?", "F1"]; onActivated: shortcuts.open() }
  // Ctrl with 1 to 8 chooses a section; Ctrl+Tab and Ctrl+Shift+Tab the next or the one before.
  Repeater {
    model: settings.sections.length
    delegate: Item {
      required property int index
      Shortcut { sequence: "Ctrl+" + (index + 1); onActivated: window.choose(index) }
    }
  }
  Shortcut { sequence: "Ctrl+Tab"; onActivated: window.choose(window.page + 1) }
  Shortcut { sequences: ["Ctrl+Shift+Tab", "Ctrl+Backtab"]; onActivated: window.choose(window.page - 1) }

  // Esc that nothing inside took (an open list or dialog takes its own) closes
  // the window.
  Item {
    anchors.fill: parent
    anchors.margins: 28
    focus: true
    Keys.onEscapePressed: window.attemptClose()

    // Declared in the order Tab follows: the rail, the bar, the section, the
    // window's close.
    TabRail {
      id: rail
      anchors.top: masthead.bottom
      anchors.topMargin: 20
      labelled: window.width >= 760
      // The tabs shorten on a short window, down to a least height.
      tabHeight: Math.max(20, Math.min(34, (window.height - 56 - masthead.height - 20) / settings.sections.length - 5))
      // A section with an item whose changes are not saved is marked.
      tabs: settings.sections.map((section, index) => ({ name: section.name, marked: settings.unsaved.includes(index) }))
      current: window.page
      onChosen: index => window.choose(index, rail.activeFocus)
    }

    PageBar {
      id: bar
      anchors { left: rail.right; leftMargin: 16; right: parent.right; top: rail.top }
      height: 44
      commands: settings.commands
      section: window.page
    }

    // The section on screen, beside the rail and under its bar.
    Rectangle {
      anchors { left: bar.left; right: parent.right; top: bar.bottom; topMargin: 16; bottom: parent.bottom }
      color: "transparent"
      border.color: Theme.line

      SettingsView {
        id: settings
        anchors { fill: parent; margins: 20 }
        page: window.page
        onWanted: index => window.page = index
        onCloseRequested: window.attemptClose()
      }
    }

    // The top line: the app's last message, while there is one, and the
    // window's close in the corner.
    Masthead {
      id: masthead
      anchors { left: parent.left; right: parent.right; top: parent.top }
      section: I18n.t("section.config")
      StatusLine { message: Eco.message; error: Eco.messageIsError; maxWidth: Math.min(480, window.width - 260); minWidth: 90; boxHeight: 28 }
      IconButton {
        name: "close"
        tip: I18n.t("settings.close").toLowerCase()
        onClicked: window.attemptClose()
      }
    }
  }

  ShortcutsDialog { id: shortcuts }
  // Closing with changes not saved saves them first, keeps editing or discards
  // them; an item whose changes another save overtook is not saved from here.
  ConfirmDialog {
    id: confirmClose
    title: I18n.t("settings.close_title")
    question: I18n.t("settings.close_question")
    confirmText: I18n.t("settings.discard_changes")
    cancelText: I18n.t("settings.keep_editing")
    onConfirmed: {
      settings.discardChanged()
      Eco.configOpen = false
    }
    TraceButton {
      dense: true
      role: "progress"
      icon: "enter"
      text: I18n.t("settings.save_close")
      enabled: !settings.conflict && !Eco.configSaving
      tip: settings.conflict ? I18n.t("settings.stale") : ""
      onClicked: {
        confirmClose.close()
        settings.saveAll(() => Eco.configOpen = false)
      }
    }
  }
}
