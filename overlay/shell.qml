import QtQuick
import Eco.Core
import Eco.Settings
import Eco.Window

// The eco window, as eco-window runs it. Shortcuts reach it through the daemon:
// see Eco.act.
OverlayPanel {
  id: overlay

  // The settings window, created the first time it opens and then kept: see
  // ConfigWindow.
  Loader {
    id: settings
    active: false
    // Not a child of the overlay: Hyprland keeps a child above its parent, which
    // would hide the overlay's dialogs under the settings when a shortcut raises it.
    sourceComponent: ConfigWindow {}
  }
  Connections {
    target: Eco
    function onConfigOpenChanged() { if (Eco.configOpen) settings.active = true }
  }
}
