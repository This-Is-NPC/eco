import QtQuick
import QtTest
import "../../overlay/Eco/Core/draft.js" as Draft

// The answers settings: the reviewer turned on takes the assistant's model
// when it has none, what names no model answers with the assistant's, and the
// files of the global context and of a slot are added, moved and removed.
TestCase {
  name: "Answers"

  function draft() {
    return {
      models: [{ name: "m", type: "chat" }, { name: "r", type: "chat" }],
      llm: { model: "m" }, kinds: ["meeting", "interview"],
      actions: [{ name: "reply" }, { name: "minutes", model: "r" }]
    }
  }

  function test_a_reviewer_turned_on_without_a_model_takes_the_assistants() {
    const d = draft()
    Draft.review(d, true)
    compare(d.reviewer, { model: "m", enabled: true })
  }

  function test_a_reviewer_keeps_its_model_on_and_off() {
    const d = draft()
    d.reviewer = { enabled: false, model: "r", verbose: true }
    Draft.review(d, true)
    compare(d.reviewer, { enabled: true, model: "r", verbose: true })
    Draft.review(d, false)
    compare(d.reviewer, { enabled: false, model: "r", verbose: true })
  }

  function test_what_names_no_model_inherits_the_assistant() {
    const d = draft()
    compare(Draft.inheriting(d), { skills: ["reply"], kinds: ["meeting", "interview"], translation: true })
    d.models[1].kinds = ["interview"]
    d.translation = { model: "r" }
    compare(Draft.inheriting(d), { skills: ["reply"], kinds: ["meeting"], translation: false })
  }

  function contexts() {
    return { context: { files: ["a.md"] }, contexts: [{ name: "work", files: ["w.md", "x.md"], kinds: [] }, { name: "home", files: [], kinds: [] }] }
  }

  function test_files_are_added_last_once_each() {
    const d = contexts()
    Draft.addFiles(Draft.contextFiles(d, ""), ["b.md", "a.md", "b.md", "c.md"])
    compare(d.context.files, ["a.md", "b.md", "c.md"])
    Draft.addFiles(Draft.contextFiles(d, "home"), ["h.md"])
    compare(d.contexts[1].files, ["h.md"])
    compare(d.contexts[0].files, ["w.md", "x.md"])
  }

  function test_a_slot_file_moves_and_is_removed_by_name() {
    const d = contexts()
    Draft.moveFile(d, "work", "x.md", -1)
    compare(d.contexts[0].files, ["x.md", "w.md"])
    Draft.moveFile(d, "work", "x.md", 1)
    compare(d.contexts[0].files, ["w.md", "x.md"])
    Draft.removeFile(d, "work", "w.md")
    compare(d.contexts[0].files, ["x.md"])
    Draft.removeFile(d, "", "a.md")
    compare(d.context.files, [])
  }
}
