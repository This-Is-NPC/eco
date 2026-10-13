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

  // With nothing registered, a model just added and not yet named serves nothing,
  // so its type can still be chosen.
  function test_a_nameless_model_serves_nothing() {
    const d = draft([{ name: "", type: "chat", base_url: "http://llm", model: "" }], "", "")
    verify(!Draft.serves(d, d.models[0], "chat", "", null))
    verify(!Draft.serves(d, d.models[0], "chat", "meeting", null))
    verify(!Draft.serves(d, d.models[0], "translation", "", null))
    const named = draft(registered, "w", "m")
    verify(Draft.serves(named, named.models[1], "chat", "meeting", null))
  }

  function test_naming_a_new_model_makes_it_no_default() {
    const d = draft([{ name: "", type: "transcription", base_url: "http://stt", model: "" }], "", "")
    Draft.rename(d, 0, "w")
    compare([d.models[0].name, d.stt.model, d.llm.model], ["w", "", ""])
  }

  function test_renaming_a_model_renames_what_names_it() {
    const d = draft(JSON.parse(JSON.stringify(registered)), "w", "m")
    d.actions = [{ name: "probe", model: "m" }, { name: "ask" }]
    d.translation = { model: "m" }
    Draft.rename(d, 1, "fast")
    compare([d.llm.model, d.stt.model, d.translation.model, d.actions[0].model, d.actions[1].model], ["fast", "w", "fast", "fast", undefined])
  }
}
