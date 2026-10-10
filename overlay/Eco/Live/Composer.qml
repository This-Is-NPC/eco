import QtQuick
import Eco.Core

ComposerSurface {
  id: composer

  property string sessionId: ""

  actions: Eco.actions
  // The session it writes to: the one given, or the one this window shows.
  readonly property string target: sessionId || (Eco.session ? Eco.session.id : "")
  // What this composer's session is answering; another session's answer never holds it.
  readonly property string answering: Eco.answering(target)
  selectedAction: answering
  busy: answering !== ""
  placeholder: compact ? I18n.t("composer.placeholder_short") : I18n.t("composer.placeholder")

  onActionChosen: name => Eco.triggerOn(target, name)
  onSubmitted: text => Eco.askOn(target, text.replace(/\n/g, " "))

  onNoted: text => Eco.noteOn(target, text)
}
