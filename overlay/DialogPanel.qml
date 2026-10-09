import QtQuick

Panel {
  property bool dialogOpen: false
  property var revealItems: []

  color: "transparent"
  border.width: 0
  onDialogOpenChanged: if (dialogOpen) {
    replayTitle()
    for (const item of revealItems) item.replay()
  }
}
