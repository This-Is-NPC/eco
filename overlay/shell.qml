import QtQuick

// The eco window, as eco-window runs it. Shortcuts reach it through the daemon:
// see Eco.act.
OverlayPanel {
  id: overlay

  // The settings window, created the first time it opens and then kept: see
  // ConfigWindow.
  Loader {
    id: settings
    active: false
    // A child of the overlay, so Hyprland keeps it above the pinned overlay.
    sourceComponent: ConfigWindow { transientParent: overlay }
  }
  Connections {
    target: Eco
    function onConfigOpenChanged() { if (Eco.configOpen) settings.active = true }
  }
}
