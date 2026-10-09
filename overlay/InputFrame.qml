import QtQuick

// InputFrame is the outline every text input sits in: a faint fill and a
// hairline that, while the input has focus, takes the accent at two pixels,
// and the error colour while what it holds is `invalid`.
Rectangle {
  property bool focused: false
  property bool invalid: false
  implicitWidth: 160
  color: Qt.alpha(Theme.foreground, 0.015)
  border.color: invalid ? Theme.error : focused ? Theme.primary : Theme.line
  border.width: focused ? 2 : 1
  Behavior on border.color { ColorAnimation { duration: 150 } }
}
