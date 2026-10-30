#include "bridge_macos.h"

#import <AppKit/AppKit.h>

#include <QString>
#include <QWindow>

void setHiddenFromCapture(QWindow *window, bool hidden) {
  // A Qt window's id on macOS is its NSView.
  NSView *view = reinterpret_cast<NSView *>(window->winId());
  view.window.sharingType = hidden ? NSWindowSharingNone : NSWindowSharingReadOnly;
}

void copyToPasteboard(const QString &text) {
  NSPasteboard *pasteboard = NSPasteboard.generalPasteboard;
  [pasteboard clearContents];
  [pasteboard setString:text.toNSString() forType:NSPasteboardTypeString];
}
