import QtQuick
import QtTest
import "../../overlay/Eco/Core/draft.js" as Draft

// A kind renamed as it is typed takes its models and context slots along only
// under a name that is its own, and a kind removed says what follows.
TestCase {
  name: "Skills"

  function draft() {
    return { participants: [], contexts: [{ name: "notes", kinds: ["talk"] }], kinds: ["meeting", "talk"],
      actions: [{ name: "a" }, { name: "b" }, { name: "c" }],
      models: [{ name: "m", type: "chat", base_url: "http://llm", model: "m", kinds: ["meeting", "talk"], translates: ["talk"] }],
      stt: { model: "", language: "auto", languages: ["auto"] }, llm: { model: "m" } }
  }

  function test_a_kind_renamed_as_typed_takes_its_models_along() {
    const d = draft()
    compare(Draft.renameKind(d, 1, "chat ", "talk"), "chat")
    compare([d.kinds, d.models[0].kinds, d.models[0].translates, d.contexts[0].kinds],
      [["meeting", "chat "], ["meeting", "chat"], ["chat"], ["chat"]])
  }

  // Typed for a moment as another kind's name, or emptied, the kind keeps its
  // models under its last name instead of merging them into the other's.
  function test_a_kind_typed_as_another_or_empty_keeps_its_models() {
    const d = draft()
    compare(Draft.renameKind(d, 1, "meeting", "talk"), "talk")
    compare(Draft.renameKind(d, 1, "", "talk"), "talk")
    compare([d.kinds, d.models[0].kinds, d.contexts[0].kinds], [["meeting", ""], ["meeting", "talk"], ["talk"]])
    compare(Draft.renameKind(d, 1, "meetings", "talk"), "meetings")
    compare([d.models[0].kinds, d.models[0].translates], [["meeting", "meetings"], ["meetings"]])
  }

  // A new kind's models name it "" until it has a name.
  function test_a_new_kind_names_its_models_once_named() {
    const d = draft()
    d.kinds.push("")
    d.models[0].kinds.push("")
    compare(Draft.renameKind(d, 2, "idea", ""), "idea")
    compare(d.models[0].kinds, ["meeting", "talk", "idea"])
  }

  function test_removing_a_kind_says_what_follows() {
    const d = draft()
    compare(Draft.kindRemoval(d, "meeting"), { heir: "talk", own: true })
    d.kinds.push("idea")
    compare(Draft.kindRemoval(d, "idea"), { heir: "", own: false })
    Draft.removeKind(d, "talk")
    compare([d.kinds, d.models[0].kinds, d.models[0].translates, d.contexts[0].kinds], [["meeting", "idea"], ["meeting"], undefined, []])
  }
}
