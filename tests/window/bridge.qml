import QtQuick
import QtQuick.Window
import EcoHost

// The bridge eco-window gives the overlay, driven by tests/window.rs: each
// line the test sends comes back with what the bridge sees; "raise" raises its
// window first, "copy" copies ECO_TEST_VALUE first; "quit" ends it.
QtObject {
  property Window shown: Window { visible: true }
  property TextFile file: TextFile {
    path: Host.env("ECO_TEST_FILE")
    onChanged: reload()
  }
  property LineSocket link: LineSocket {
    path: Host.env("ECO_TEST_SOCKET")
    token: Host.env("ECO_TEST_TOKEN")
    onReceived: line => {
      if (line === "quit")
        Qt.quit()
      else {
        if (line === "raise")
          Host.raise(shown)
        if (line === "copy")
          Host.copy(Host.env("ECO_TEST_VALUE"))
        send(JSON.stringify({ line: line, value: Host.env("ECO_TEST_VALUE"), text: file.text, pid: Host.pid,
          path: Host.localPath(Host.env("ECO_TEST_URL")), foreground: Host.foreground, microphone: Host.microphone, monospace: Host.monospaceFamily,
          system_dark: Host.systemDark, system_theme_directory: Host.systemThemeDirectory }))
      }
    }
  }
}
