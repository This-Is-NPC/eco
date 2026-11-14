#pragma once

#include <QString>

// The Android calls of the window host: where the app keeps the daemon's files,
// and starting the foreground service that runs the daemon. The service is
// window/android/src/eco/app/DaemonService.java; nothing else lives here.
namespace AndroidBridge {
// The directory for everything eco keeps (ECO_DATA_DIR of the daemon).
QString dataDir();
// The directory of the socket (ECO_RUNTIME_DIR of the daemon).
QString runtimeDir();
// The daemon's socket, which the window connects to.
QString socketPath();
// Names the socket in ECO_SOCKET, asks for the microphone and, once allowed,
// for notifications, and starts the service.
void start();
} // namespace AndroidBridge
