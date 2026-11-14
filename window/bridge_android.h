#pragma once

#include <QObject>
#include <QString>

// The Android calls of the window host: where the app keeps the daemon's files,
// and starting the foreground service that runs the daemon. The service is
// window/android/src/eco/app/DaemonService.java; nothing else lives here.
namespace AndroidBridge {
// What keeps the daemon from running, as the window shows it: "" when nothing
// does, "models.preparing" while the service gets the models, "models.failed"
// when it could not and will try again. The host reports it and decides
// nothing.
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
// Takes the status the service reports, from any thread.
void report(const QString &status);
// Names the socket in ECO_SOCKET, asks for the microphone and, once allowed,
// for notifications, and starts the service.
void start();
} // namespace AndroidBridge
