.pragma library

// The Markdown an answer card draws. Qt fetches the URL of a Markdown image
// while it lays the text out, so an answer never reaches Text.MarkdownText
// with one: images become their alt text.

// `text` with the Markdown marks a stream left open closed, so a reply still
// arriving draws as it will once complete: an open code fence, then the bold
// and inline code marks, innermost first; a mark with nothing after it yet is
// dropped instead.
function closed(text) {
  if ((text.match(/^```/gm) || []).length % 2)
    return text + "\n```"
  let shown = text.replace(/\s+$/, "")
  const open = ["**", "`"].filter(mark => shown.split(mark).length % 2 === 0)
    .map(mark => ({ mark: mark, at: shown.lastIndexOf(mark) }))
    .sort((a, b) => b.at - a.at)
  for (const left of open)
    shown = shown.slice(left.at + left.mark.length).trim() === "" ? shown.slice(0, left.at).replace(/\s+$/, "") : shown + left.mark
  return shown
}

// The alt text of an HTML <img> tag, or "" when it has none.
function alt(tag) {
  const found = tag.match(/\balt\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))/i)
  return found ? found[1] || found[2] || found[3] || "" : ""
}

// `text` outside code: inline and reference images replaced by their alt
// text, any other `![` escaped so it draws as written, and <img> tags
// replaced by their alt text.
function prose(text) {
  return text
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/!\[([^\]]*)\]\[[^\]]*\]/g, "$1")
    .replace(/!\[/g, "!\\[")
    .replace(/<img\b[^>]*>?/gi, alt)
}

// `text` with no image Qt would load; code blocks and inline code are kept as written.
function imageless(text) {
  return text.split(/(```[\s\S]*?(?:```|$)|`[^`\n]*`)/)
    .map((part, i) => i % 2 ? part : prose(part))
    .join("")
}
