import QtQuick
import QtTest
import "../../overlay/Eco/Core/draft.js" as Draft

// The settings draft saves with no model registered and no default chosen; a
// default it names is a registered model of its type.
TestCase {
  name: "Draft"

  function draft(models, transcriber, assistant) {
    return { participants: [], secrets: [], providers: [{ name: "stt", base_url: "http://stt" }, { name: "llm", base_url: "http://llm" }],
      models: models, contexts: [], actions: [], kinds: ["meeting"],
      stt: { model: transcriber, language: "auto", languages: ["auto"] }, llm: { model: assistant } }
  }

  readonly property var registered: [
    { name: "w", type: "transcription", provider: "stt", model: "whisper-1" },
    { name: "m", type: "chat", provider: "llm", model: "m" }
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

  readonly property var shipped: ({
    secrets: [{ name: "OPENROUTER", env: "OPENROUTER_API_KEY" }],
    providers: [{ name: "OPENROUTER", base_url: "https://openrouter.ai/api/v1", secret: "OPENROUTER" }],
    models: [{ name: "gpt", type: "chat", provider: "OPENROUTER", model: "openai/gpt" },
      { name: "nova", type: "transcription", provider: "OPENROUTER", model: "nova-3" }]
  })

  function test_a_new_model_starts_from_the_provider_of_the_last_one_of_its_type() {
    Draft.usePresets(shipped)
    const d = draft(JSON.parse(JSON.stringify(registered)), "", "")
    compare(Draft.provider(d, "chat"), "llm")
    compare(Draft.provider(draft([], "", ""), "chat"), "OPENROUTER")
    Draft.retype(d, 0, "chat")
    compare(d.models[0], { name: "w", type: "chat", provider: "stt", model: "whisper-1" })
    Draft.usePresets({ secrets: [], providers: [], models: [] })
  }

  // The models eco ships are named like the user's own, unless the user has one by that name.
  function test_a_default_may_name_a_preset_model() {
    Draft.usePresets(shipped)
    compare(paths(draft([], "nova", "gpt")), [])
    compare(Draft.named(draft(registered, "", ""), "chat"), ["m", "gpt"])
    compare(paths(draft([], "gpt", "nova")), ["stt.model", "llm.model"])
    verify(Draft.chats(draft([], "", "")))
    compare(Draft.users(draft([], "", "gpt"), "gpt").map(use => use.role), ["assistant", "translation", "kind"])
    Draft.usePresets({ secrets: [], providers: [], models: [] })
  }

  function test_a_model_needs_a_listed_provider_and_a_provider_a_listed_secret() {
    Draft.usePresets(shipped)
    const d = draft([{ name: "a", type: "chat", provider: "gone", model: "x" }, { name: "b", type: "chat", provider: "OPENROUTER", model: "x" }], "", "")
    d.providers.push({ name: "p", base_url: "http://p", secret: "gone" }, { name: "q", base_url: " ", secret: "OPENROUTER" })
    compare(paths(d), ["providers.2.secret", "providers.3.base_url", "models.0.provider"])
    Draft.usePresets({ secrets: [], providers: [], models: [] })
  }

  function test_a_secret_has_exactly_one_reference() {
    const d = draft([], "", "")
    d.secrets = [{ name: "a", env: "KEY" }, { name: "b", env: "" }, { name: "c", env: "K", omapass: "x" }, { name: "d", credential: "a b" },
      { name: "e", omapass: "acct" }]
    compare(paths(d), ["secrets.1.source", "secrets.2.source", "secrets.3.source"])
  }

  // What names a secret or provider follows its typed name, never an empty or another item's.
  function test_renaming_a_secret_or_provider_renames_what_names_it() {
    const d = draft([{ name: "a", type: "chat", provider: "llm", model: "x" }], "", "")
    d.secrets = [{ name: "k", env: "KEY" }, { name: "j", env: "J" }]
    d.providers[1].secret = "k"
    compare(Draft.renameItem(d, "secrets", 0, "key2", "k"), "key2")
    compare(d.providers[1].secret, "key2")
    compare(Draft.renameItem(d, "secrets", 0, "j", "key2"), "key2")
    compare(Draft.renameItem(d, "secrets", 0, "", "key2"), "key2")
    compare(Draft.renameItem(d, "providers", 1, "remote", "llm"), "remote")
    compare(d.models[0].provider, "remote")
    compare(Draft.referrers(d, "secrets", "key2"), ["remote"])
    compare(Draft.referrers(d, "providers", "remote"), ["a"])
  }

  // A secret of the user's reading a preset secret's variable takes its place.
  function test_a_secret_reading_a_preset_variable_replaces_the_preset() {
    Draft.usePresets(shipped)
    const d = draft([], "", "")
    d.secrets = [{ name: "OPENROUTER_API_KEY", env: "OPENROUTER_API_KEY" }]
    compare(Draft.shipped(d, "secrets").length, 0)
    compare(Draft.listed(d, "secrets"), ["OPENROUTER_API_KEY"])
    compare(Draft.shippedSecret(d, shipped.providers[0]), "OPENROUTER_API_KEY")
    compare(Draft.referrers(d, "secrets", "OPENROUTER_API_KEY", true), ["OPENROUTER"])
    compare(Draft.shipped(draft([], "", ""), "secrets").length, 1)
    Draft.usePresets({ secrets: [], providers: [], models: [] })
  }

  function test_what_the_presets_use_counts_only_when_asked() {
    Draft.usePresets(shipped)
    const d = draft([], "", "")
    compare(Draft.referrers(d, "providers", "OPENROUTER"), [])
    compare(Draft.referrers(d, "providers", "OPENROUTER", true), ["gpt", "nova"])
    compare(Draft.referrers(d, "secrets", "OPENROUTER", true), ["OPENROUTER"])
    Draft.usePresets({ secrets: [], providers: [], models: [] })
  }

  // With nothing registered, a model just added and not yet named serves nothing,
  // so its type can still be chosen.
  function test_a_nameless_model_serves_nothing() {
    const d = draft([{ name: "", type: "chat", provider: "llm", model: "" }], "", "")
    verify(!Draft.serves(d, d.models[0], "chat", "", null))
    verify(!Draft.serves(d, d.models[0], "chat", "meeting", null))
    verify(!Draft.serves(d, d.models[0], "translation", "", null))
    const named = draft(registered, "w", "m")
    verify(Draft.serves(named, named.models[1], "chat", "meeting", null))
  }

  function test_naming_a_new_model_makes_it_no_default() {
    const d = draft([{ name: "", type: "transcription", provider: "stt", model: "" }], "", "")
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
    d.models.push({ name: "x", type: "chat", provider: "llm", model: "x" })
    d.actions = [{ name: "probe", model: "m" }, { name: "other", model: "x" }]
    let following = "m"
    for (const text of ["", "x", "x2"])
      following = Draft.rename(d, 1, text, following)
    compare([following, d.models[1].name, d.llm.model, d.actions[0].model, d.actions[1].model], ["x2", "x2", "x2", "x2", "x"])
    compare(Draft.rename(d, 1, " ", following), "x2")
    compare(d.llm.model, "x2")
  }

  // A model's users name what answers with it, as the daemon picks it: the
  // defaults, the reviewer, the skills naming it and each kind once, with its uses.
  function test_the_users_of_a_model_are_what_answers_with_it() {
    const d = draft(JSON.parse(JSON.stringify(registered)), "w", "m")
    d.kinds = ["meeting", "talk"]
    d.models.push({ name: "x", type: "chat", provider: "llm", model: "x", kinds: ["talk"] })
    d.actions = [{ name: "probe", model: "m" }, { name: "ask" }]
    d.reviewer = { enabled: true, model: "m" }
    compare(Draft.users(d, "m"), [{ role: "assistant" }, { role: "translation" }, { role: "reviewer" },
      { role: "skill", action: "probe" }, { role: "kind", kind: "meeting", uses: ["chat", "translation"] },
      { role: "kind", kind: "talk", uses: ["translation"] }])
    compare(Draft.users(d, "x"), [{ role: "kind", kind: "talk", uses: ["chat"] }])
    compare(Draft.users(d, "w"), [{ role: "transcriber" }, { role: "kind", kind: "meeting", uses: ["transcription"] },
      { role: "kind", kind: "talk", uses: ["transcription"] }])
    compare(Draft.users(d, ""), [])
    compare(Draft.users(d, "gone"), [])
    d.reviewer.enabled = false
    compare(Draft.users(d, "m")[2], { role: "reviewer" })
  }

  // The roles left empty show the model the daemon picked, marked automatic;
  // a role the user chose keeps its own, and the draft itself stays empty.
  function test_the_automatic_defaults_are_shown_not_saved() {
    const d = draft(JSON.parse(JSON.stringify(registered)), "", "")
    d.llm.model = "m"
    const picked = { transcription: "w", chat: "m", translation: "m", review: "m",
      automatic: ["transcription", "translation", "review"] }
    const shown = Draft.resolved(d, picked)
    compare(shown.automatic, ["transcription", "translation", "review"])
    compare([shown.draft.stt.model, shown.draft.translation.model, shown.draft.reviewer.model, shown.draft.reviewer.enabled], ["w", "m", "m", false])
    compare(Draft.defaults(shown.draft, "m").map(entry => entry.use), ["chat", "translation", "review"])
    compare([d.stt.model, d.translation, d.reviewer], ["", undefined, undefined])
    d.translation = { model: "m" }
    compare(Draft.resolved(d, picked).automatic, ["transcription", "review"])
  }

  // A model's defaults are what it answers when nothing more specific picks a
  // model; translation follows the chat default until one of its own is set.
  function test_a_model_is_made_the_default_for_each_use() {
    const d = draft(JSON.parse(JSON.stringify(registered)), "", "")
    d.models.push({ name: "x", type: "chat", provider: "llm", model: "x" })
    compare(Draft.defaults(d, "m"), [])
    Draft.makeDefault(d, "chat", "m")
    compare(d.llm.model, "m")
    compare(Draft.defaults(d, "m"), [{ use: "chat", inherited: false }, { use: "translation", inherited: true }])
    Draft.makeDefault(d, "translation", "x")
    compare(d.translation, { model: "x" })
    compare(Draft.defaults(d, "m"), [{ use: "chat", inherited: false }])
    compare(Draft.defaults(d, "x"), [{ use: "translation", inherited: false }])
    Draft.makeDefault(d, "translation", "m")
    compare(Draft.defaults(d, "m"), [{ use: "chat", inherited: false }, { use: "translation", inherited: false }])
    Draft.makeDefault(d, "transcription", "w")
    compare([d.stt.model, Draft.defaults(d, "w")], ["w", [{ use: "transcription", inherited: false }]])
    compare(Draft.problems(d), [])
  }

  // The reviewer's model is a default too: made one, review stays as it was.
  function test_a_model_is_made_the_default_for_review() {
    const d = draft(JSON.parse(JSON.stringify(registered)), "", "m")
    d.models.push({ name: "x", type: "chat", provider: "llm", model: "x" })
    Draft.makeDefault(d, "review", "x")
    compare(d.reviewer.enabled, false)
    compare(d.reviewer.model, "x")
    compare(Draft.defaults(d, "x"), [{ use: "review", inherited: false }])
    d.reviewer.enabled = true
    Draft.makeDefault(d, "review", "m")
    compare([d.reviewer.enabled, d.reviewer.model], [true, "m"])
    compare(Draft.defaults(d, "x"), [])
    compare(Draft.defaults(d, "m"), [{ use: "chat", inherited: false }, { use: "translation", inherited: true }, { use: "review", inherited: false }])
    compare(Draft.problems(d), [])
  }

  function test_a_renamed_kind_follows_into_models_and_slots() {
    const d = draft([{ name: "m", type: "chat", provider: "llm", model: "m", kinds: ["meeting", "talk"], translates: ["talk"] }], "", "m")
    d.contexts = [{ name: "notes", kinds: ["talk"] }]
    Draft.followKind(d, "talk", "chat")
    compare([d.models[0].kinds, d.models[0].translates, d.contexts[0].kinds], [["meeting", "chat"], ["chat"], ["chat"]])
    Draft.followKind(d, "chat", null)
    compare([d.models[0].kinds, d.models[0].translates, d.contexts[0].kinds], [["meeting"], undefined, []])
  }

  function test_a_draft_is_saved_trimmed_and_serving_only_its_kinds() {
    const d = draft([{ name: " m ", type: "chat", provider: "llm", model: " id ", kinds: ["meeting", "gone"], translates: ["gone"] }], " w ", " m ")
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
    added.models.unshift({ name: "x", type: "chat", provider: "llm", model: "x" })
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
    models.models.push({ name: "x", type: "chat", provider: "llm", model: "x" })
    compare(Draft.rebased(models, base, edited, ["kinds.0"], null, [serve]), null)
    const people = JSON.parse(JSON.stringify(base))
    people.participants = [{ name: "Ana", devices: [] }]
    const again = Draft.rebased(people, base, edited, ["kinds.0"], null, [serve])
    compare([again.models[1].kinds, again.participants.length], [["meeting"], 1])
  }
}
