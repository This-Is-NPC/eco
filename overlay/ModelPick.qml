import QtQuick
import "draft.js" as Draft

// ModelPick picks the model a skill, or sessions of a kind, use for `use` —
// "chat", "transcription" or "translation" — or the default, which names the
// model it inherits and reads dim. The caller applies `picked`.
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
  describe: name => name || I18n.t("settings.default_model", { name: Draft.answering(pick.draft, pick.use, "", null).name })
  dim: !Draft.answering(draft, use, action ? "" : kind, action).own
}
