import QtQuick
import Quickshell
import Quickshell.Io

ShellRoot {
  OverlayPanel { id: overlay }

  // The settings window, created the first time it opens and then kept: see
  // ConfigWindow.
  LazyLoader {
    id: settings
    // A child of the overlay, so Hyprland keeps it above the pinned overlay.
    ConfigWindow { parentWindow: overlay }
  }
  Connections {
    target: Eco
    function onConfigOpenChanged() { if (Eco.configOpen) settings.active = true }
  }

  // `quickshell ipc --path <overlay> call eco <function>`, e.g. from a Hyprland shortcut.
  IpcHandler {
    target: "eco"
    function toggleConfig(): void { Eco.toggleConfig() }
    function newSession(): void { Eco.newSessionRequested() }
    function sessions(): void { if (Eco.session === null) Eco.openAllHistory() }
    function importFile(path: string): void { Eco.importRequested(path) }
  }
}
