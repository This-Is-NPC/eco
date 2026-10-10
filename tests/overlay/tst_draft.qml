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

  readonly property var presets: ({
    chat: [{ name: "OPENROUTER", values: { base_url: "https://openrouter.ai/api/v1", api_key_env: "OPENROUTER_API_KEY" } }],
    transcription: [{ name: "DEEPGRAM", values: { base_url: "wss://api.deepgram.com/v1/listen", api_key_env: "DEEPGRAM_API_KEY" } }]
  })

  function test_a_new_model_keeps_the_key_source_of_the_last_one() {
    const keyed = [{ name: "c", type: "chat", base_url: "http://llm", model: "m", api_key_credential: "llm" }]
    compare(Draft.provider(draft(keyed, "", ""), "chat", presets), { base_url: "http://llm", api_key_credential: "llm" })
    const d = draft(keyed, "", "")
    Draft.retype(d, 0, "transcription", presets)
    compare(d.models[0].api_key_credential, "llm")
  }
}
