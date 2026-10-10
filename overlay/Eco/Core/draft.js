.pragma library

// Reads and edits of the settings draft that several views share.

// A copy of the draft's translation settings, {} when it has none.
function translation(draft) {
  return Object.assign({}, draft.translation)
}

// The names of the draft's registered models of `type`, "chat" or "transcription".
function named(draft, type) {
  return draft.models.filter(model => model.type === type && model.name).map(model => model.name)
}

// The model sessions of `kind` choose for `use` — "transcription", "chat" or
// "translation" — or "" when they keep the default.
function serving(draft, use, kind) {
  const own = draft.models.find(model => use === "translation" ? (model.translates || []).includes(kind)
    : model.type === use && (model.kinds || []).includes(kind))
  return own ? own.name : ""
}

// Make sessions of `kind` use the model `name` for `use` in the draft `d`, or the default for "".
function serve(d, use, kind, name) {
  const field = use === "translation" ? "translates" : "kinds"
  const type = use === "transcription" ? "transcription" : "chat"
  d.models.forEach(model => {
    if (model.type !== type)
      return
    const kinds = (model[field] || []).filter(k => k !== kind)
    if (model.name === name)
      kinds.push(kind)
    if (kinds.length)
      model[field] = kinds
    else
      delete model[field]
  })
}

// The model that really answers, as the daemon picks it: for `use` —
// "chat", "transcription" or "translation" — of the skill `action` (an entry
// of the draft's actions, or null) in a session of `kind` ("" for any). A chat
// answer takes the skill's model, then the kind's, then the assistant's; a
// translation, the kind's, the translation default, then the assistant's; a
// transcription, the kind's, then the default. Returns {name, own}, `own`
// when the most specific of the skill, the kind and the default asked names it
// itself rather than inheriting it.
function answering(draft, use, kind, action) {
  const type = use === "transcription" ? "transcription" : "chat"
  const known = name => !!name && draft.models.some(model => model.type === type && model.name === name)
  const steps = []
  if (action)
    steps.push(action.model || "")
  if (kind)
    steps.push(serving(draft, use, kind))
  if (use === "translation")
    steps.push(draft.translation && draft.translation.model || "")
  steps.push(use === "transcription" ? draft.stt.model : draft.llm.model)
  const at = steps.findIndex(known)
  return { name: at < 0 ? "" : steps[at], own: at === 0 }
}

// The provider a model of `type` in the draft starts from: the base URL and key
// source of the draft's last model of that type, or else of the type's first
// preset in `presets` ({chat: [...], transcription: [...]}).
function provider(draft, type, presets) {
  const last = (draft.models || []).filter(model => model.type === type).pop()
  const from = last || presets[type][0].values
  const fields = { base_url: from.base_url }
  if (from.api_key_omapass !== undefined)
    fields.api_key_omapass = from.api_key_omapass
  else if (from.api_key_env !== undefined)
    fields.api_key_env = from.api_key_env
  return fields
}

// Make the model at `index` in the draft `d` the other `type`, keeping its name
// and model id. Its base URL stays when it fits the type — a preset of that type
// uses it, or no preset does and it is not a streaming wss:// one for chat —;
// otherwise the provider and key come from `provider`. A chat model's reasoning,
// extra fields and translations leave with its type.
function retype(d, index, type, presets) {
  const entry = d.models[index]
  const url = entry.base_url || ""
  const preset = list => list.some(p => p.values.base_url === url)
  const fits = preset(presets[type]) || (!preset(presets.chat) && !preset(presets.transcription)
    && !(type === "chat" && url.startsWith("wss://")))
  const kept = { name: entry.name, type: type, model: entry.model || "" }
  if (fits)
    ["base_url", "api_key_env", "api_key_omapass"].forEach(key => { if (entry[key] !== undefined) kept[key] = entry[key] })
  d.models[index] = Object.assign(kept, fits ? {} : provider(d, type, presets))
}

// Remove the model at `index` from the draft `d`: the skills and translation
// default that named it fall back, and a reviewer on it turns off.
function removeModel(d, index) {
  const name = d.models[index].name
  d.models.splice(index, 1)
  if (!name)
    return
  if (d.reviewer && d.reviewer.model === name) {
    d.reviewer.model = ""
    d.reviewer.enabled = false
  }
  if (d.translation && d.translation.model === name)
    delete d.translation.model
  d.actions.forEach(action => { if (action.model === name) delete action.model })
}

// What removing the model at `index` changes: `moves`, each a skill (`action`,
// its name) or a kind's `use` of `kind` ("" for the translation default) with
// the model that answers it `to` afterwards, as `answering` picks it; and
// `reviewerOff`, when the reviewer ran on it.
function removal(draft, index) {
  const entry = draft.models[index]
  const moves = []
  if (!entry.name)
    return { moves: moves, reviewerOff: false }
  const after = JSON.parse(JSON.stringify(draft))
  removeModel(after, index)
  const move = (use, kind, action) => moves.push({ use: use, kind: kind, action: action ? action.name : "",
    to: answering(after, use, kind, action).name })
  draft.actions.forEach((action, i) => { if (action.model === entry.name) move("chat", "", after.actions[i]) })
  ;(entry.kinds || []).forEach(kind => move(entry.type, kind, null))
  ;(entry.translates || []).forEach(kind => move("translation", kind, null))
  if (draft.translation && draft.translation.model === entry.name)
    move("translation", "", null)
  return { moves: moves, reviewerOff: !!draft.reviewer && !!draft.reviewer.enabled && draft.reviewer.model === entry.name }
}

// What keeps the draft `d` from saving, each on the field it concerns, in the
// order of the settings tabs: {page, path, key, typed}. `path` names the field
// ("actions.2", "models.0.base_url"), `key` its message; `typed` marks what is
// wrong in what was typed — a repeated name, a space — rather than missing.
// Names compare trimmed, as they are saved.
function problems(d) {
  const found = []
  const add = (page, path, key, typed) => found.push({ page: page, path: path, key: key, typed: !!typed })
  // Every name nonempty and unique; with `spaced` false, also without spaces.
  const names = (page, list, path, spaced) => {
    const trimmed = list.map(name => name.trim())
    trimmed.forEach((name, i) => {
      if (!name)
        add(page, path(i), "settings.error_empty", false)
      else if (!spaced && /\s/.test(name))
        add(page, path(i), "settings.error_spaces", true)
      else if (trimmed.filter(other => other === name).length > 1)
        add(page, path(i), "settings.error_taken", true)
    })
  }
  const typed = type => d.models.filter(model => model.type === type).map(model => model.name.trim())
  names(0, d.participants.map(person => person.name), i => "participants." + i, true)
  names(1, d.models.map(model => model.name), i => "models." + i + ".name", true)
  d.models.forEach((model, i) => {
    if (!model.base_url.trim())
      add(1, "models." + i + ".base_url", "settings.error_base_url")
    if (!model.model.trim())
      add(1, "models." + i + ".model", "settings.error_model_id")
  })
  const languages = d.stt.languages.map(code => code.trim().toLowerCase())
  if (!languages.length || languages.some(code => code !== "auto" && !/^[a-z]{2,3}$/.test(code))
      || new Set(languages).size !== languages.length || !languages.includes(d.stt.language))
    add(2, "stt.languages", "settings.invalid_languages")
  if (!typed("transcription").includes(d.stt.model.trim()))
    add(2, "stt.model", "settings.invalid_transcriber")
  if (!typed("chat").includes(d.llm.model.trim()))
    add(3, "llm.model", "settings.invalid_assistant")
  names(3, (d.contexts || []).map(slot => slot.name), i => "contexts." + i, true)
  if (d.reviewer && d.reviewer.enabled && !typed("chat").includes(d.reviewer.model.trim()))
    add(3, "reviewer.model", "settings.invalid_reviewer")
  names(4, d.actions.map(action => action.name), i => "actions." + i, false)
  names(6, d.kinds, i => "kinds." + i, true)
  return found
}
