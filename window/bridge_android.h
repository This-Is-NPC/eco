#pragma once

#include <QObject>
#include <QString>

// The Android calls of the window host: where the app keeps the daemon's files,
// and starting the foreground service that runs the daemon. The service is
// window/android/src/eco/app/DaemonService.java; nothing else lives here.
namespace AndroidBridge {
// What keeps the daemon from running, as the window shows it: "" when nothing
// does, "microphone.denied" (Android asks again) or "microphone.blocked"
// (Android will not: the app's settings page is the way), "models.preparing"
// while the service gets the models, "models.failed" when it could not and
// will try again. The host reports it and decides nothing.
class Status : public QObject {
  Q_OBJECT
public:
  static Status &instance();
  const QString &value() const { return current; }
  void set(const QString &status);

Q_SIGNALS:
  void changed();

private:
  QString current;
};

// The directory for everything eco keeps (ECO_DATA_DIR of the daemon).
QString dataDir();
// The directory of the socket (ECO_RUNTIME_DIR of the daemon).
QString runtimeDir();
// The daemon's socket, which the window connects to.
QString socketPath();
// The status a microphone answer leaves: `canAskAgain` is whether Android
// still shows its question.
QString microphoneStatus(Qt::PermissionStatus microphone, bool canAskAgain);
// Takes the status the service reports, from any thread.
void report(const QString &status);
// The action of the status: asks for the microphone again, or opens the app's
// settings page where Android will not ask.
void act();
// Names the socket in ECO_SOCKET, asks for the microphone and, once allowed,
// for notifications, and starts the service. The window shows `Status` while
// the microphone is refused, and the service starts when it is allowed.
void start();
} // namespace AndroidBridge
