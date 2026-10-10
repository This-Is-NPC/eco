#pragma once

#include <QCoreApplication>
#include <QObject>
#include <QString>
#include <QUrl>

#ifdef Q_OS_WIN
#include <QClipboard>
#include <QEvent>
#include <QGuiApplication>
#include <QWindow>
#ifndef NOMINMAX
#define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#else
#include <QProcess>
#include <QStringList>
#endif

// Host is the process as QML sees it: its pid, its environment, local paths and
// the clipboard; on Windows also leaving its windows out of screen capture.
class Host : public QObject {
  Q_OBJECT
  Q_PROPERTY(qint64 pid READ pid CONSTANT)

public:
  explicit Host(QObject *parent = nullptr) : QObject(parent) {
#ifdef Q_OS_WIN
    qApp->installEventFilter(this);
#endif
  }

  qint64 pid() const { return QCoreApplication::applicationPid(); }

  // The value of the environment variable `name`, or "" when it is unset.
  Q_INVOKABLE QString env(const QString &name) const { return qEnvironmentVariable(name.toUtf8().constData()); }

  // The local path a file: URL names, as file dialogs and drops give them.
  Q_INVOKABLE QString localPath(const QUrl &url) const { return url.toLocalFile(); }

  // Puts `text` on the clipboard so that it stays there after this process
  // exits: on Windows through Qt, which hands it to the system as the process
  // exits; elsewhere through wl-copy, which keeps serving it.
  Q_INVOKABLE void copy(const QString &text) const {
#ifdef Q_OS_WIN
    QGuiApplication::clipboard()->setText(text);
#else
    QProcess::startDetached(QStringLiteral("wl-copy"), {QStringLiteral("--"), text});
#endif
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
#endif
};
