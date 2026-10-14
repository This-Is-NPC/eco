import QtQuick

// FocusKeeper gives the keyboard back to what had it before `popup` opened,
// once it closes — also when the window was not active as it opened — unless
// what was picked in it gave the keyboard to something else.
QtObject {
  id: keeper
  required property var popup
  property Item opener: null
  // What has the window's keyboard; nothing while the window is inactive.
  readonly property Item focused: popup.parent ? popup.parent.Window.activeFocusItem : null
  // The last thing outside the popup that had it, kept while the window is inactive.
  property Item last: null
  onFocusedChanged: if (focused && !popup.visible) last = focused
  // Whether the keyboard is on something outside the popup and its window's
  // own content item: given there by what was picked.
  function moved() {
    const window = popup.parent ? popup.parent.Window.window : null
    if (!focused || !window || focused === window.contentItem)
      return false
    for (let at = focused; at; at = at.parent)
      if (at === popup.contentItem)
        return false
    return focused !== opener
  }
  readonly property Connections watch: Connections {
    target: keeper.popup
    function onAboutToShow() { keeper.opener = keeper.focused || keeper.last }
    function onClosed() {
      // Unless what it opened moved away from the opener's view, which then
      // takes no input, or the keyboard went elsewhere while it closed.
      if (keeper.opener && keeper.opener.visible && keeper.opener.enabled && !keeper.moved())
        keeper.opener.forceActiveFocus()
      keeper.opener = null
    }
  }
}
