import QtQuick
import Eco.Kit

// ViewStage is a Stage whose view is made the first time it is shown. A `kept`
// view stays made after; another is let go once it has left the screen. The
// view's `active` follows `shown` from just after it is made, so a view that
// acts when it turns active acts on its first showing too.
Stage {
  id: stage

  property Component view
  property bool kept: false
  // The view, or null while it is not made.
  readonly property Item item: loader.item

  Loader {
    id: loader
    anchors.fill: parent
    focus: stage.shown
    active: stage.shown || (!stage.kept && stage.visible)
    // Made once shown, a kept view is not let go.
    Binding on active {
      when: stage.kept && stage.shown
      value: true
      restoreMode: Binding.RestoreNone
    }
    sourceComponent: stage.view
    onLoaded: item.active = Qt.binding(() => stage.shown)
  }
}
