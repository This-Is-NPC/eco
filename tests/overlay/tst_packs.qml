import QtQuick
import QtTest
import "../../overlay/Eco/Core/packs.js" as Packs

// A language pack read again takes its new texts and keeps those it no longer
// has, as when an update puts another version's packs under an open window.
TestCase {
  name: "Packs"

  function test_a_first_read_is_the_file() {
    compare(Packs.reread(undefined, '{"a": "A"}'), { a: "A" })
  }

  function test_a_reread_takes_the_new_texts() {
    compare(Packs.reread({ a: "A" }, '{"a": "B", "c": "C"}'), { a: "B", c: "C" })
  }

  function test_a_key_the_new_file_lacks_keeps_its_text() {
    compare(Packs.reread({ a: "A", "update.stage.waiting": "W" }, '{"a": "B"}'),
            { a: "B", "update.stage.waiting": "W" })
  }

  function test_a_file_that_is_not_json_throws() {
    let threw = false
    try { Packs.reread({ a: "A" }, '{"a": ') } catch (error) { threw = true }
    verify(threw)
  }
}
