import QtQuick
import QtTest
import "../../overlay/Eco/Core/draft.js" as Draft

// The settings draft saves with no model registered and no default chosen; a
// default it names is a registered model of its type.
TestCase {
  name: "Draft"

  function draft(models, transcriber, assistant) {
    return { participants: [], models: models, contexts: [], actions: [], kinds: ["meeting"],
      stt: { model: transcriber, language: "auto", languages: ["auto"] }, llm: { model: assistant } }
  }

  readonly property var registered: [
    { name: "w", type: "transcription", base_url: "http://stt", model: "whisper-1" },
    { name: "m", type: "chat", base_url: "http://llm", model: "m" }
  ]

  function paths(d) {
    return Draft.problems(d).map(found => found.path)
  }

  function test_no_models_and_no_defaults_save() {
    compare(paths(draft([], "", "")), [])
  }

  function test_no_default_chosen_saves() {
    compare(paths(draft(registered, "", "")), [])
  }

  function test_registered_defaults_save() {
    compare(paths(draft(registered, "w", "m")), [])
  }

  function test_a_default_naming_no_model_of_its_type_does_not_save() {
    compare(paths(draft(registered, "m", "w")), ["stt.model", "llm.model"])
    compare(paths(draft([], "gone", "gone")), ["stt.model", "llm.model"])
  }
}
