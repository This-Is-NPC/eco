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

// Whether the draft has a chat model registered: answers, skills and
// translation need one.
function chats(draft) {
  return named(draft, "chat").length > 0
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

// Whether the registered model `entry` answers `use` of the skill `action` in
// a session of `kind`, as `answering` picks it; one with no name answers nothing.
function serves(draft, entry, use, kind, action) {
  return !!entry.name && answering(draft, use, kind, action).name === entry.name
}

// Name the model at `index` in the draft `d` as `text` is typed. The defaults,
// reviewer, translation and skills that named it name it `from`, the last name
// it had that was nonempty and no other model's ("" for a model never named,
// which none of them names); they follow the typed name, trimmed, only once it
// is such a name too, so a model never takes another's. Returns the name they
// hold now.
function rename(d, index, text, from) {
  d.models[index].name = text
  const name = text.trim()
  if (!name || name === from || d.models.some((model, i) => i !== index && model.name.trim() === name))
    return from
  if (!from)
    return name
  if (d.llm.model === from) d.llm.model = name
  if (d.stt.model === from) d.stt.model = name
  if (d.reviewer && d.reviewer.model === from) d.reviewer.model = name
  if (d.translation && d.translation.model === from) d.translation.model = name
  d.actions.forEach(action => { if (action.model === from) action.model = name })
  return name
}

// The provider a model of `type` in the draft starts from: the base URL and key
// source of the draft's last model of that type, or else of the type's first
// preset in `presets` ({chat: [...], transcription: [...]}).
function provider(draft, type, presets) {
  const last = (draft.models || []).filter(model => model.type === type).pop()
  const from = last || presets[type][0].values
  const fields = { base_url: from.base_url }
  if (from.api_key_credential !== undefined)
    fields.api_key_credential = from.api_key_credential
  else if (from.api_key_omapass !== undefined)
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
    ["base_url", "api_key_env", "api_key_omapass", "api_key_credential"].forEach(key => { if (entry[key] !== undefined) kept[key] = entry[key] })
  d.models[index] = Object.assign(kept, fits ? {} : provider(d, type, presets))
}

// What uses the model named `name` in the draft now, as `answering` picks it:
// [{role, kind, uses, action}], `role` one of "assistant", "transcriber",
// "translation" (the translation default), "reviewer", "skill" (`action` its
// name) or "kind" (`kind` once, with its `uses`: "chat", "transcription" and
// "translation"). A model with no name, or none of that name, is used by nothing.
function users(draft, name) {
  const entry = draft.models.find(model => model.name === name)
  if (!name || !entry)
    return []
  const found = []
  const answers = (use, kind) => serves(draft, entry, use, kind, null)
  const chat = entry.type === "chat"
  if (chat && answers("chat", ""))
    found.push({ role: "assistant" })
  if (!chat && answers("transcription", ""))
    found.push({ role: "transcriber" })
  if (chat && answers("translation", ""))
    found.push({ role: "translation" })
  if (chat && draft.reviewer && draft.reviewer.enabled && draft.reviewer.model === name)
    found.push({ role: "reviewer" })
  draft.actions.forEach(action => { if (action.model === name) found.push({ role: "skill", action: action.name }) })
  const uses = chat ? ["chat", "translation"] : ["transcription"]
  draft.kinds.forEach(kind => {
    const served = uses.filter(use => answers(use, kind))
    if (served.length > 0)
      found.push({ role: "kind", kind: kind, uses: served })
  })
  return found
}

// Replace the kind `from` by `to` in the draft `d`'s models (`kinds`,
// `translates`) and context slots, or drop it there for a null `to`.
function followKind(d, from, to) {
  const swap = list => list.map(kind => kind === from ? to : kind).filter(kind => kind !== null)
  d.models.forEach(model => {
    for (const field of ["kinds", "translates"]) {
      if (!model[field])
        continue
      model[field] = swap(model[field])
      if (!model[field].length)
        delete model[field]
    }
  })
  ;(d.contexts || []).forEach(slot => slot.kinds = swap(slot.kinds))
}

// The edit of an item made again on `config`, the saved config that changed
// since the edit began from `base`: a copy of `config` with `make` (the new
// item, or null) and each of `changes` applied, in order. Null when a list
// the edit reached changed there — one of those the item's `paths` live in
// ("models.2" lives in "models") or any other `draft`, the edit as it is now,
// changed — since the changes made again may then fall on another item.
function rebased(config, base, draft, paths, make, changes) {
  const reached = Object.keys(draft).filter(list => JSON.stringify(draft[list]) !== JSON.stringify(base[list]))
  const lists = paths.map(path => path.split(".")[0]).concat(reached)
  if (lists.some(list => JSON.stringify(config[list]) !== JSON.stringify(base[list])))
    return null
  const next = JSON.parse(JSON.stringify(config))
  if (make)
    make(next)
  changes.forEach(change => change(next))
  return next
}

// A copy of the draft `d` as it is saved: names, model ids and the names
// that refer to models trimmed, languages trimmed and lowercase, and each
// model serving only the kinds there are.
function normalised(d) {
  const next = JSON.parse(JSON.stringify(d))
  const trim = name => name.trim()
  next.participants.forEach(person => person.name = trim(person.name))
  next.kinds = next.kinds.map(trim)
  next.stt.languages = next.stt.languages.map(code => code.trim().toLowerCase())
  next.actions.forEach(action => {
    action.name = trim(action.name)
    if (action.model)
      action.model = trim(action.model)
  })
  next.models.forEach(model => {
    model.name = trim(model.name)
    model.model = trim(model.model)
  })
  next.llm.model = trim(next.llm.model)
  next.stt.model = trim(next.stt.model)
  if (next.translation && next.translation.model)
    next.translation.model = trim(next.translation.model)
  if (next.reviewer)
    next.reviewer.model = trim(next.reviewer.model)
  next.contexts = (next.contexts || []).map(slot => Object.assign(slot, { name: trim(slot.name) }))
  next.models.forEach(model => {
    for (const field of ["kinds", "translates"]) {
      if (model[field])
        model[field] = model[field].filter(kind => next.kinds.includes(kind))
      if (model[field] && !model[field].length)
        delete model[field]
    }
  })
  return next
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
// order of the settings tabs: {path, key, typed}. `path` names the field
// ("actions.2", "models.0.base_url"), `key` its message; `typed` marks what is
// wrong in what was typed — a repeated name, a space — rather than missing.
// Names compare trimmed, as they are saved.
function problems(d) {
  const found = []
  const add = (path, key, typed) => found.push({ path: path, key: key, typed: !!typed })
  // Every name nonempty and unique; with `spaced` false, also without spaces.
  const names = (list, path, spaced) => {
    const trimmed = list.map(name => name.trim())
    trimmed.forEach((name, i) => {
      if (!name)
        add(path(i), "settings.error_empty", false)
      else if (!spaced && /\s/.test(name))
        add(path(i), "settings.error_spaces", true)
      else if (trimmed.filter(other => other === name).length > 1)
        add(path(i), "settings.error_taken", true)
    })
  }
  const typed = type => d.models.filter(model => model.type === type).map(model => model.name.trim())
  names(d.participants.map(person => person.name), i => "participants." + i, true)
  names(d.models.map(model => model.name), i => "models." + i + ".name", true)
  d.models.forEach((model, i) => {
    if (!model.base_url.trim())
      add("models." + i + ".base_url", "settings.error_base_url")
    if (!model.model.trim())
      add("models." + i + ".model", "settings.error_model_id")
  })
  const languages = d.stt.languages.map(code => code.trim().toLowerCase())
  if (!languages.length || languages.some(code => code !== "auto" && !/^[a-z]{2,3}$/.test(code))
      || new Set(languages).size !== languages.length || !languages.includes(d.stt.language))
    add("stt.languages", "settings.invalid_languages")
  // A default may name no model, as before any is registered; one it names is registered.
  if (d.stt.model.trim() && !typed("transcription").includes(d.stt.model.trim()))
    add("stt.model", "settings.invalid_transcriber")
  if (d.llm.model.trim() && !typed("chat").includes(d.llm.model.trim()))
    add("llm.model", "settings.invalid_assistant")
  names((d.contexts || []).map(slot => slot.name), i => "contexts." + i, true)
  if (d.reviewer && d.reviewer.enabled && !typed("chat").includes(d.reviewer.model.trim()))
    add("reviewer.model", "settings.invalid_reviewer")
  names(d.actions.map(action => action.name), i => "actions." + i, false)
  names(d.kinds, i => "kinds." + i, true)
  return found
}

// Give the device `id` to the audio source at `index` of the draft `d`,
// taking it from whoever had it; with `index` -1 nobody is heard through it.
function assign(d, id, index) {
  d.participants.forEach(person => person.devices = person.devices.filter(device => device !== id))
  if (index >= 0)
    d.participants[index].devices.push(id)
}

// Make the audio source at `index` of the draft `d` the user, the only one;
// when it was already, nobody is.
function makeUser(d, index) {
  const was = d.participants[index].user
  d.participants.forEach((person, i) => person.user = i === index && !was)
}

// What removing the audio source at `index` of `draft` loses, given the
// `devices` there are ({id, kind}): the devices it was heard through,
// `noOutput` when no other source has an output device, `noUser` when it was the user.
function sourceRemoval(draft, index, devices) {
  const source = draft.participants[index]
  const outputs = person => person.devices.some(id => devices.some(device => device.id === id && device.kind === "output"))
  return { unheard: source.devices.slice(), noOutput: outputs(source) && !draft.participants.some((person, i) => i !== index && outputs(person)),
    noUser: !!source.user }
}

// Move the entry at `from` of `list` by `step` places: -1 up, 1 down.
function shift(list, from, step) {
  list.splice(from + step, 0, list.splice(from, 1)[0])
}

// Move the skill named `name` in the draft `d` by `step` places: -1 up, 1 down.
function moveSkill(d, name, step) {
  shift(d.actions, d.actions.findIndex(action => action.name === name), step)
}

// Offer the spoken language `code` in the draft `d`, last, unless it is offered.
function addLanguage(d, code) {
  if (!d.stt.languages.includes(code))
    d.stt.languages.push(code)
}

// Move the offered spoken language `code` in the draft `d` by `step` places.
function moveLanguage(d, code, step) {
  shift(d.stt.languages, d.stt.languages.indexOf(code), step)
}

// The language that becomes the default when `code` is no longer offered in
// `draft`: the first one left when `code` was the default, else "".
function languageHeir(draft, code) {
  return draft.stt.language === code ? draft.stt.languages.filter(other => other !== code)[0] || "" : ""
}

// Stop offering the spoken language `code` in the draft `d`; when it was the
// default, the first one left becomes the default.
function removeLanguage(d, code) {
  const heir = languageHeir(d, code)
  d.stt.languages.splice(d.stt.languages.indexOf(code), 1)
  if (heir)
    d.stt.language = heir
}

// Name the kind at `index` in the draft `d` as `text` is typed. Its models and
// context slots name it `from`, the last name it had that was nonempty and no
// other kind's; they follow the typed name, trimmed, only once it is such a name
// too, so a kind never takes another's. Returns the name they hold now.
function renameKind(d, index, text, from) {
  d.kinds[index] = text
  const name = text.trim()
  if (!name || name === from || d.kinds.some((kind, i) => i !== index && kind.trim() === name))
    return from
  followKind(d, from, name)
  return name
}

// What removing the kind `kind` from the draft changes: `heir`, the kind that
// becomes the default ("" when it was not the default), and `own`, whether
// models or context slots chose it.
function kindRemoval(draft, kind) {
  return {
    heir: draft.kinds[0] === kind ? draft.kinds[1] || "" : "",
    own: draft.models.some(model => (model.kinds || []).includes(kind) || (model.translates || []).includes(kind))
      || (draft.contexts || []).some(slot => slot.kinds.includes(kind))
  }
}

// Remove the kind `kind` from the draft `d`, its models and its context slots.
function removeKind(d, kind) {
  d.kinds.splice(d.kinds.indexOf(kind), 1)
  followKind(d, kind, null)
}

// Turn the draft `d`'s reviewer on or off; turned on with no model, it
// reviews with the assistant's.
function review(d, on) {
  const reviewer = d.reviewer || { model: "" }
  reviewer.enabled = on
  if (on && !reviewer.model)
    reviewer.model = d.llm.model
  d.reviewer = reviewer
}

// What answers with the assistant's model because it names none of its own,
// as `answering` picks it: {skills, kinds, translation}, the names of the
// skills and the kinds, and whether the translation default does.
function inheriting(draft) {
  return {
    skills: draft.actions.filter(action => !answering(draft, "chat", "", action).own).map(action => action.name),
    kinds: draft.kinds.filter(kind => !serving(draft, "chat", kind)),
    translation: !answering(draft, "translation", "", null).own
  }
}

// The files of a context in the draft `d`, sent in this order: the global
// context's when `slot` is "", else those of the context slot named `slot`.
function contextFiles(d, slot) {
  return slot === "" ? d.context.files : d.contexts.find(entry => entry.name === slot).files
}

// Add `paths` to the file list `list`, last, those not in it yet.
function addFiles(list, paths) {
  for (const path of paths)
    if (!list.includes(path))
      list.push(path)
}

// Move the file `path` of the context `slot` in the draft `d` by `step` places.
function moveFile(d, slot, path, step) {
  const files = contextFiles(d, slot)
  shift(files, files.indexOf(path), step)
}

// Stop sending the file `path` with the context `slot` of the draft `d`.
function removeFile(d, slot, path) {
  const files = contextFiles(d, slot)
  files.splice(files.indexOf(path), 1)
}

// What taking the device `id` from the audio source named `name` of `draft`
// leaves, given the `devices` there are ({id, kind}): `empty` when the source
// is heard through no other device, `noOutput` when `id` was the last output
// device any source had.
function unassignment(draft, name, id, devices) {
  const output = device => devices.some(entry => entry.id === device && entry.kind === "output")
  const source = draft.participants.find(person => person.name === name)
  return { empty: source.devices.every(device => device === id),
    noOutput: output(id) && !draft.participants.some(person => person.devices.some(device => device !== id && output(device))) }
}

// Show the device `id` in `color` in the draft `d`, or in its automatic colour for "".
function recolor(d, id, color) {
  d.colors = d.colors || {}
  if (color)
    d.colors[id] = color
  else
    delete d.colors[id]
}
