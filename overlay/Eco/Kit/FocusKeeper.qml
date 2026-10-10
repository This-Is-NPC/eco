import QtQuick

// FocusKeeper gives the keyboard back to what had it before `popup` opened,
// once it closes — also when the window was not active as it opened.
QtObject {
  id: keeper
  required property var popup
  property Item opener: null
  // What has the window's keyboard; nothing while the window is inactive.
  readonly property Item focused: popup.parent ? popup.parent.Window.activeFocusItem : null
  // The last thing outside the popup that had it, kept while the window is inactive.
  property Item last: null
  onFocusedChanged: if (focused && !popup.visible) last = focused
  readonly property Connections watch: Connections {
    target: keeper.popup
    function onAboutToShow() { keeper.opener = keeper.focused || keeper.last }
    function onClosed() {
      // Unless what it opened moved away from the opener's view, which then
      // takes no input.
      if (keeper.opener && keeper.opener.visible && keeper.opener.enabled)
        keeper.opener.forceActiveFocus()
      keeper.opener = null
    }
  }
}
