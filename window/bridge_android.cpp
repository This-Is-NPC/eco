#include "bridge_android.h"

#include <QCoreApplication>
#include <QDir>
#include <QGuiApplication>
#include <QJniEnvironment>
#include <QJniObject>
#include <QPermissions>

namespace {
const char service[] = "eco/app/DaemonService";

bool serviceStarted = false;

QJniObject context() { return QNativeInterface::QAndroidApplication::context(); }

QString filesDir() {
  return context().callObjectMethod("getFilesDir", "()Ljava/io/File;").callObjectMethod<jstring>("getAbsolutePath").toString();
}

void startService() {
  QJniObject::callStaticMethod<void>(service, "start", "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
                                     context().object(), QJniObject::fromString(AndroidBridge::dataDir()).object(),
                                     QJniObject::fromString(AndroidBridge::runtimeDir()).object());
}

// Whether Android still shows its question for the microphone.
bool canAskAgain() {
  return context().callMethod<jboolean>("shouldShowRequestPermissionRationale", "(Ljava/lang/String;)Z",
                                        QJniObject::fromString(QStringLiteral("android.permission.RECORD_AUDIO")).object());
}

void reportNative(JNIEnv *, jclass, jstring status) { AndroidBridge::report(QJniObject(status).toString()); }

// The service's reports come through a native method of its class.
void listenToService() {
  QJniEnvironment env;
  if (!env.registerNativeMethods(service, {{"report", "(Ljava/lang/String;)V", reinterpret_cast<void *>(&reportNative)}}))
    qWarning("the service's reports are not heard: the window cannot say why the daemon is down");
}

// A microphone service needs the microphone allowed to start at all: starts the
// service once it is, and not twice.
void begin() {
  if (serviceStarted)
    return;
  serviceStarted = true;
  AndroidBridge::Status::instance().set({});
  QJniObject::callStaticMethod<void>(service, "askNotifications", "(Landroid/app/Activity;)V", context().object());
  startService();
}

void answered(const QPermission &microphone) {
  if (microphone.status() == Qt::PermissionStatus::Granted)
    begin();
  else
    AndroidBridge::Status::instance().set(AndroidBridge::microphoneStatus(microphone.status(), canAskAgain()));
}

void openSettings() {
  const QJniObject uri = QJniObject::callStaticObjectMethod(
      "android/net/Uri", "fromParts", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Landroid/net/Uri;",
      QJniObject::fromString(QStringLiteral("package")).object(), context().callObjectMethod<jstring>("getPackageName").object(),
      nullptr);
  const QJniObject intent("android/content/Intent", "(Ljava/lang/String;Landroid/net/Uri;)V",
                          QJniObject::fromString(QStringLiteral("android.settings.APPLICATION_DETAILS_SETTINGS")).object(),
                          uri.object());
  context().callMethod<void>("startActivity", "(Landroid/content/Intent;)V", intent.object());
  if (QJniEnvironment().checkAndClearExceptions())
    qWarning("the app's settings page did not open");
}
} // namespace

namespace AndroidBridge {
Status &Status::instance() {
  static Status status;
  return status;
}

void Status::set(const QString &status) {
  if (status == current)
    return;
  current = status;
  qInfo("status: %s", qPrintable(status));
  Q_EMIT changed();
}

QString dataDir() { return QDir(filesDir()).filePath(QStringLiteral("eco")); }

QString runtimeDir() { return QDir(filesDir()).filePath(QStringLiteral("run")); }

QString socketPath() { return QDir(runtimeDir()).filePath(QStringLiteral("eco.sock")); }

QString microphoneStatus(Qt::PermissionStatus microphone, bool canAskAgain) {
  if (microphone != Qt::PermissionStatus::Denied)
    return {};
  return canAskAgain ? QStringLiteral("microphone.denied") : QStringLiteral("microphone.blocked");
}

void report(const QString &status) {
  QMetaObject::invokeMethod(&Status::instance(), [status] { Status::instance().set(status); }, Qt::QueuedConnection);
}

void act() {
  if (Status::instance().value() == QLatin1String("microphone.blocked"))
    openSettings();
  else if (Status::instance().value() == QLatin1String("microphone.denied"))
    qApp->requestPermission(QMicrophonePermission{}, qApp, answered);
}

void start() {
  qputenv("ECO_SOCKET", socketPath().toLocal8Bit());
  listenToService();
  qApp->requestPermission(QMicrophonePermission{}, qApp, answered);
  // Back from the settings page, the microphone may be allowed.
  QObject::connect(qApp, &QGuiApplication::applicationStateChanged, qApp, [](Qt::ApplicationState state) {
    if (state == Qt::ApplicationActive && qApp->checkPermission(QMicrophonePermission{}) == Qt::PermissionStatus::Granted)
      begin();
  });
}
} // namespace AndroidBridge
