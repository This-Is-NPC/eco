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
    compare(Draft.rename(d, 0, "w", ""), "w")
    compare([d.models[0].name, d.stt.model, d.llm.model], ["w", "", ""])
  }

  function test_renaming_a_model_renames_what_names_it() {
    const d = draft(JSON.parse(JSON.stringify(registered)), "w", "m")
    d.actions = [{ name: "probe", model: "m" }, { name: "ask" }]
    d.translation = { model: "m" }
    compare(Draft.rename(d, 1, "fast", "m"), "fast")
    compare([d.llm.model, d.stt.model, d.translation.model, d.actions[0].model, d.actions[1].model], ["fast", "w", "fast", "fast", undefined])
  }

  // Typed a letter at a time, the name what names the model follows is never
  // empty and never another model's: cleared, it is kept; passing through
  // another model's name, neither model's users move.
  function test_a_model_renamed_as_typed_keeps_its_users_and_takes_none() {
    const d = draft(JSON.parse(JSON.stringify(registered)), "w", "m")
    d.models.push({ name: "x", type: "chat", base_url: "http://llm", model: "x" })
    d.actions = [{ name: "probe", model: "m" }, { name: "other", model: "x" }]
    let following = "m"
    for (const text of ["", "x", "x2"])
      following = Draft.rename(d, 1, text, following)
    compare([following, d.models[1].name, d.llm.model, d.actions[0].model, d.actions[1].model], ["x2", "x2", "x2", "x2", "x"])
    compare(Draft.rename(d, 1, " ", following), "x2")
    compare(d.llm.model, "x2")
  }

  // A model's users name what answers with it, as the daemon picks it: the
  // defaults, the reviewer, the skills naming it and each kind's uses.
  function test_the_users_of_a_model_are_what_answers_with_it() {
    const d = draft(JSON.parse(JSON.stringify(registered)), "w", "m")
    d.kinds = ["meeting", "talk"]
    d.models.push({ name: "x", type: "chat", base_url: "http://llm", model: "x", kinds: ["talk"] })
    d.actions = [{ name: "probe", model: "m" }, { name: "ask" }]
    d.reviewer = { enabled: true, model: "m" }
    compare(Draft.users(d, "m"), [{ role: "assistant" }, { role: "translation" }, { role: "reviewer" },
      { role: "skill", action: "probe" }, { role: "kind", kind: "meeting", use: "chat" },
      { role: "kind", kind: "meeting", use: "translation" }, { role: "kind", kind: "talk", use: "translation" }])
    compare(Draft.users(d, "x"), [{ role: "kind", kind: "talk", use: "chat" }])
    compare(Draft.users(d, "w"), [{ role: "transcriber" }, { role: "kind", kind: "meeting", use: "transcription" },
      { role: "kind", kind: "talk", use: "transcription" }])
    compare(Draft.users(d, ""), [])
    compare(Draft.users(d, "gone"), [])
  }

  function test_a_renamed_kind_follows_into_models_and_slots() {
    const d = draft([{ name: "m", type: "chat", base_url: "http://llm", model: "m", kinds: ["meeting", "talk"], translates: ["talk"] }], "", "m")
    d.contexts = [{ name: "notes", kinds: ["talk"] }]
    Draft.followKind(d, "talk", "chat")
    compare([d.models[0].kinds, d.models[0].translates, d.contexts[0].kinds], [["meeting", "chat"], ["chat"], ["chat"]])
    Draft.followKind(d, "chat", null)
    compare([d.models[0].kinds, d.models[0].translates, d.contexts[0].kinds], [["meeting"], undefined, []])
  }

  function test_a_draft_is_saved_trimmed_and_serving_only_its_kinds() {
    const d = draft([{ name: " m ", type: "chat", base_url: "http://llm", model: " id ", kinds: ["meeting", "gone"], translates: ["gone"] }], " w ", " m ")
    d.participants = [{ name: " Ana " }]
    d.kinds = [" meeting "]
    d.stt.languages = [" EN "]
    d.actions = [{ name: " ask ", model: " m " }]
    d.translation = { model: " m " }
    d.reviewer = { enabled: true, model: " m " }
    d.contexts = [{ name: " notes ", kinds: [] }]
    const next = Draft.normalised(d)
    compare([next.participants[0].name, next.kinds, next.stt.languages, next.actions[0].name, next.actions[0].model,
      next.models[0].name, next.models[0].model, next.llm.model, next.stt.model, next.translation.model,
      next.reviewer.model, next.contexts[0].name, next.models[0].kinds, next.models[0].translates],
      ["Ana", ["meeting"], ["en"], "ask", "m", "m", "id", "m", "w", "m", "m", "notes", ["meeting"], undefined])
    compare(d.models[0].name, " m ")
  }

  // An item's edit is made again on a saved config another save changed, only
  // while the lists the item lives in, and every list its edit reached, are
  // untouched there.
  function test_an_edit_is_made_again_on_a_newer_config_unless_its_list_changed() {
    const base = draft(JSON.parse(JSON.stringify(registered)), "w", "m")
    const rename = d => Draft.rename(d, 1, "fast", "m")
    const edited = JSON.parse(JSON.stringify(base))
    rename(edited)
    const elsewhere = JSON.parse(JSON.stringify(base))
    elsewhere.kinds = ["meeting", "idea"]
    const again = Draft.rebased(elsewhere, base, edited, ["models.1"], null, [rename])
    compare([again.models[1].name, again.llm.model, again.kinds], ["fast", "fast", ["meeting", "idea"]])
    const added = JSON.parse(JSON.stringify(base))
    added.models.unshift({ name: "x", type: "chat", base_url: "http://x", model: "x" })
    compare(Draft.rebased(added, base, edited, ["models.1"], null, [rename]), null)
  }

  // A kind's edit that picks its models reaches the models list: a save there
  // elsewhere puts it in conflict, while a save of a list it never reached does not.
  function test_an_edit_is_not_made_again_when_a_list_it_reached_changed() {
    const base = draft(JSON.parse(JSON.stringify(registered)), "w", "m")
    const serve = d => Draft.serve(d, "chat", "meeting", "m")
    const edited = JSON.parse(JSON.stringify(base))
    serve(edited)
    const models = JSON.parse(JSON.stringify(base))
    models.models.push({ name: "x", type: "chat", base_url: "http://x", model: "x" })
    compare(Draft.rebased(models, base, edited, ["kinds.0"], null, [serve]), null)
    const people = JSON.parse(JSON.stringify(base))
    people.participants = [{ name: "Ana", devices: [] }]
    const again = Draft.rebased(people, base, edited, ["kinds.0"], null, [serve])
    compare([again.models[1].kinds, again.participants.length], [["meeting"], 1])
  }
}
