import QtQuick
import Eco.Core
import Eco.Kit

// ModelPick picks the model a skill, or sessions of a kind, use for `use` —
// "chat", "transcription" or "translation" — or the default, which names the
// model it inherits and reads dim, or says there is no chat model; a chat or
// translation pick is off, saying why, until one is registered. The caller
// applies `picked`.
PickRow {
  id: pick

  required property var draft
  property string use: "chat"
  property string kind
  // The index of the skill, or -1 for a kind.
  property int skill: -1
  readonly property var action: skill >= 0 ? draft.actions[skill] : null

  options: ["", ...Draft.named(draft, use === "transcription" ? "transcription" : "chat")]
  current: action ? (action.model || "") : Draft.serving(draft, use, kind)
  // The default names the model it inherits; with no chat model, it says there is none.
  describe: name => {
    const inherited = Draft.answering(pick.draft, pick.use, "", null).name
    return name || (inherited || pick.use === "transcription" ? I18n.t("settings.default_model", { name: inherited }) : I18n.t("settings.no_chat"))
  }
  reason: use === "transcription" || Draft.chats(draft) ? "" : I18n.t("settings.needs_chat")
  dim: !Draft.answering(draft, use, action ? "" : kind, action).own
}
