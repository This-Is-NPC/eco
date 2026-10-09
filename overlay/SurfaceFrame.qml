import QtQuick

Rectangle {
  id: surface

  property string variant: "outline"
  property color tone: Theme.primary
  property bool active: false

  // The surface's own fill, for a frame that shows it only sometimes.
  readonly property color fill: variant === "filled" ? Qt.alpha(tone, 0.07) : Qt.alpha(Theme.foreground, 0.015)

  color: fill
  border.color: active ? tone : variant === "filled" ? Qt.alpha(tone, 0.45) : Theme.line

  Behavior on color { ColorAnimation { duration: 150 } }
  Behavior on border.color { ColorAnimation { duration: 150 } }
}
