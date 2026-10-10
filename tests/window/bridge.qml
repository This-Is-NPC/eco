import QtQuick
import EcoHost

// The bridge eco-window gives the overlay, driven by tests/window.rs: each
// line the test sends comes back with what the bridge sees; "quit" ends it.
QtObject {
  property TextFile file: TextFile {
    path: Host.env("ECO_TEST_FILE")
    onChanged: reload()
  }
  property LineSocket link: LineSocket {
    path: Host.env("ECO_TEST_SOCKET")
    onReceived: line => {
      if (line === "quit")
        Qt.quit()
      else
        send(JSON.stringify({ line: line, value: Host.env("ECO_TEST_VALUE"), text: file.text }))
    }
  }
}
