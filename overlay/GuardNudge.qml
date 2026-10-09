import QtQuick

SequentialAnimation {
  id: guard

  required property QtObject targetObject

  NumberAnimation { target: guard.targetObject; property: "x"; from: 0; to: -5; duration: 70 }
  NumberAnimation { target: guard.targetObject; property: "x"; to: 5; duration: 100 }
  NumberAnimation { target: guard.targetObject; property: "x"; to: 0; duration: 90 }
}
