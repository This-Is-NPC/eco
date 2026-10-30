#pragma once

class QString;
class QWindow;

// Leaves `window` out of screen capture, or shows it in it again, through its
// NSWindow's sharing type.
void setHiddenFromCapture(QWindow *window, bool hidden);

// Puts `text` on the general pasteboard as its data, not a promise, so it
// stays there once this process exits, killed or not.
void copyToPasteboard(const QString &text);
