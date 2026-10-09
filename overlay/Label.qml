import QtQuick

// Label is every text in eco: the terminal's monospace font, plain text
// unless told otherwise, so what the daemon sends never turns into markup.
Text {
  font.family: Theme.fontFamily
  font.pixelSize: 13
  color: Theme.foreground
  textFormat: Text.PlainText
}
