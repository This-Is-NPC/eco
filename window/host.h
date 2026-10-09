#pragma once

#include <QCoreApplication>
#include <QObject>
#include <QProcess>
#include <QString>
#include <QStringList>

// Host is the process as QML sees it: its environment, its pid, the clipboard
// and its windows' screen-share setting on Hyprland.
class Host : public QObject {
  Q_OBJECT
  Q_PROPERTY(qint64 processId READ processId CONSTANT)

public:
  using QObject::QObject;

  qint64 processId() const { return QCoreApplication::applicationPid(); }

  // The value of the environment variable `name`, or "" when it is unset.
  Q_INVOKABLE QString env(const QString &name) const { return qEnvironmentVariable(name.toUtf8().constData()); }

  // Puts `text` on the Wayland clipboard through wl-copy, which keeps serving it
  // after this process exits.
  Q_INVOKABLE void copy(const QString &text) const {
    QProcess::startDetached(QStringLiteral("wl-copy"), {QStringLiteral("--"), text});
  }

  // Sets Hyprland's no_screen_share on every window of this process, on or off.
  Q_INVOKABLE void hideFromScreenShare(bool hidden) const {
    const QString lua = QStringLiteral(R"((function()
  for _, w in ipairs(hl.get_windows()) do
    if w.pid == %1 then
      hl.dispatch(hl.dsp.window.set_prop({ prop = "no_screen_share", value = "%2", window = "address:" .. w.address }))
    end
  end
  return hl.dsp.no_op()
end)())")
                            .arg(processId())
                            .arg(hidden ? 1 : 0);
    QProcess::startDetached(QStringLiteral("hyprctl"), {QStringLiteral("dispatch"), lua});
  }
};
