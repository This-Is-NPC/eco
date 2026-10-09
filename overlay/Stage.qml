import QtQuick

// Stage is one of a window's views. Navigating forward, the view being left
// slides out to the left while the new one comes in from the right, so a
// switch reads as one short motion.
Item {
  id: stage
  property bool shown: false
  property real offset: 0
  function revealTree(item) {
    if (!item)
      return
    if (item.soft === true && item.revealOnStage && item.replay)
      item.replay()
    for (const child of item.children || [])
      revealTree(child)
  }

  function revealContent() {
    for (const child of stage.children)
      revealTree(child)
  }

  visible: opacity > 0
  // A view on its way out takes no input, so the keyboard moves on with the view.
  enabled: shown
  opacity: 0
  transform: Translate { x: stage.offset }

  Component.onCompleted: if (shown) { opacity = 1; Qt.callLater(() => stage.revealContent()) }
  // Only one motion at a time: a view shown and hidden at once must end hidden.
  onShownChanged: {
    if (shown) {
      leave.stop()
      enter.restart()
      Qt.callLater(() => { if (stage.shown) stage.revealContent() })
    } else {
      enter.stop()
      leave.restart()
    }
  }

  ParallelAnimation {
    id: enter
    NumberAnimation { target: stage; property: "opacity"; to: 1; duration: 220; easing.type: Easing.OutCubic }
    NumberAnimation { target: stage; property: "offset"; from: 28; to: 0; duration: 280; easing.type: Easing.OutCubic }
  }
  ParallelAnimation {
    id: leave
    NumberAnimation { target: stage; property: "opacity"; to: 0; duration: 160 }
    NumberAnimation { target: stage; property: "offset"; to: -28; duration: 200; easing.type: Easing.InCubic }
  }
}
