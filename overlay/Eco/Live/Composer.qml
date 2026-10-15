import QtQuick
import Eco.Core

ComposerSurface {
  id: composer

  // The session it writes to: {id, kind}.
  property var session: null

  readonly property string target: session ? session.id : ""
  readonly property var models: Eco.modelsOf(session ? session.kind : "")
  actions: Eco.actions
  ready: models.actions
  asks: models.chat !== ""
  reason: I18n.t("error.chat.none", { kind: Eco.kindName(session ? session.kind : "") })
  // What this composer's session is answering; another session's answer never holds it.
  readonly property string answering: Eco.answering(target)
  selectedAction: answering
  busy: answering !== ""
  placeholder: I18n.t((asks ? "composer.placeholder" : ready.length ? "composer.placeholder_note" : "composer.placeholder_note_only")
    + (compact ? "_short" : ""))

  onActionChosen: name => Eco.triggerOn(target, name)
  onSubmitted: text => Eco.askOn(target, text.replace(/\n/g, " "))

  onNoted: text => Eco.noteOn(target, text)
}
