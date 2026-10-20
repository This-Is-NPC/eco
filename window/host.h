#pragma once

#include <QCoreApplication>
#include <QObject>
#include <QString>
#include <QUrl>

#include <QDir>
#include <QGuiApplication>
#include <QStyleHints>
#include <QWindow>

#ifdef Q_OS_WIN
#include <QClipboard>
#include <QEvent>
#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <ole2.h>
#else
#include <QProcess>
#include <QStringList>
#endif

// Host is the process as QML sees it: its pid, its environment, local paths,
// the clipboard, raising a window and whether the system's foreground is one of
// its windows; on Windows also leaving its windows out of screen capture.
class Host : public QObject {
  Q_OBJECT
  Q_PROPERTY(qint64 pid READ pid CONSTANT)
  Q_PROPERTY(bool systemDark READ systemDark NOTIFY systemThemeChanged)
  Q_PROPERTY(QString systemThemeDirectory READ systemThemeDirectory CONSTANT)
  Q_PROPERTY(bool foreground READ foreground NOTIFY foregroundChanged)

public:
  explicit Host(QObject *parent = nullptr) : QObject(parent) {
    connect(QGuiApplication::styleHints(), &QStyleHints::colorSchemeChanged, this, &Host::systemThemeChanged);
#ifdef Q_OS_WIN
    qApp->installEventFilter(this);
    instance = this;
    inFront = ownsForeground();
    hook = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, nullptr, &Host::foregroundMoved, 0, 0,
                           WINEVENT_OUTOFCONTEXT);
#endif
  }

#ifdef Q_OS_WIN
  ~Host() override {
    if (hook)
      UnhookWinEvent(hook);
    instance = nullptr;
  }
#endif

  qint64 pid() const { return QCoreApplication::applicationPid(); }

  bool systemDark() const { return QGuiApplication::styleHints()->colorScheme() != Qt::ColorScheme::Light; }

  QString systemThemeDirectory() const {
#ifdef Q_OS_WIN
    return {};
#else
    QString state = qEnvironmentVariable("XDG_STATE_HOME");
    if (state.isEmpty())
      state = QDir::homePath() + QStringLiteral("/.local/state");
    return state + QStringLiteral("/omarchy/current");
#endif
  }

  // Whether the window in front of the system is one of this process's. On
  // Windows Qt calls a window active that the system left behind another
  // program, its taskbar button flashing (docs/design.md §15); elsewhere Qt's
  // word holds, and this is always true.
#ifdef Q_OS_WIN
  bool foreground() const { return inFront; }
#else
  bool foreground() const { return true; }
#endif

  // The value of the environment variable `name`, or "" when it is unset.
  Q_INVOKABLE QString env(const QString &name) const { return qEnvironmentVariable(name.toUtf8().constData()); }

  // The local path a file: URL names, as file dialogs and drops give them.
  Q_INVOKABLE QString localPath(const QUrl &url) const { return url.toLocalFile(); }

  // Puts `text` on the clipboard so that it stays there after this process
  // exits: on Windows through Qt, its data handed to the system at once, so
  // that it survives even a process killed; elsewhere through wl-copy, which
  // keeps serving it.
  Q_INVOKABLE void copy(const QString &text) const {
#ifdef Q_OS_WIN
    QGuiApplication::clipboard()->setText(text);
    OleFlushClipboard();
#else
    QProcess::startDetached(QStringLiteral("wl-copy"), {QStringLiteral("--"), text});
#endif
  }

  // Brings `window` to the front with the keyboard, as the daemon's window
  // control asks (docs/design.md §15).
  Q_INVOKABLE void raise(QWindow *window) const {
    if (!window)
      return;
#ifdef Q_OS_WIN
    // Windows hands the foreground only to the process that has it or had the
    // last input, and this one, started by the daemon in the background, has
    // neither: SetForegroundWindow alone flashes the taskbar button. Attached
    // to the input of the foreground window's thread, this thread may take it,
    // the workaround Qt itself applies under AlwaysActivateWindow. A hung
    // foreground thread would hang this one once attached, so it is asked first.
    const HWND own = reinterpret_cast<HWND>(window->winId());
    const HWND front = GetForegroundWindow();
    if (front && front != own &&
        SendMessageTimeoutW(front, WM_NULL, 0, 0, SMTO_BLOCK | SMTO_ABORTIFHUNG, 1000, nullptr)) {
      const DWORD theirs = GetWindowThreadProcessId(front, nullptr);
      const DWORD ours = GetCurrentThreadId();
      AttachThreadInput(theirs, ours, TRUE);
      BringWindowToTop(own);
      SetForegroundWindow(own);
      AttachThreadInput(theirs, ours, FALSE);
    }
#endif
    window->raise();
    window->requestActivate();
  }

#ifdef Q_OS_WIN
  // Leaves every window of this process out of screen capture, or shows them
  // in it again; while left out, so is every window it shows later.
  Q_INVOKABLE void hideFromCapture(bool hidden) {
    hiding = hidden;
    for (QWindow *window : QGuiApplication::topLevelWindows())
      exclude(window);
  }

  bool eventFilter(QObject *watched, QEvent *event) override {
    if (event->type() == QEvent::Show)
      if (auto *window = qobject_cast<QWindow *>(watched); window && window->isTopLevel())
        exclude(window);
    return false;
  }

private:
  // A window without a native one yet gets the affinity once it shows.
  void exclude(QWindow *window) const {
    if (window->handle())
      SetWindowDisplayAffinity(reinterpret_cast<HWND>(window->winId()),
                               hiding ? WDA_EXCLUDEFROMCAPTURE : WDA_NONE);
  }

  bool hiding = false;

  // Whether the foreground window belongs to this process.
  static bool ownsForeground() {
    DWORD owner = 0;
    if (const HWND window = GetForegroundWindow())
      GetWindowThreadProcessId(window, &owner);
    return owner == GetCurrentProcessId();
  }

  // The system moved the foreground: called on this thread, by its message loop.
  static void CALLBACK foregroundMoved(HWINEVENTHOOK, DWORD, HWND, LONG, LONG, DWORD, DWORD) {
    if (!instance)
      return;
    const bool now = ownsForeground();
    if (now != instance->inFront) {
      instance->inFront = now;
      Q_EMIT instance->foregroundChanged();
    }
  }

  static inline Host *instance = nullptr;
  HWINEVENTHOOK hook = nullptr;
  bool inFront = false;
#endif

Q_SIGNALS:
  void foregroundChanged();
  void systemThemeChanged();
};
