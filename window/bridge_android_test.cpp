// The bridge's Android methods, run on the emulator: the directories it gives
// the daemon, the socket the window connects to, the environment it sets, and
// the status the window shows.

#include <QFileInfo>
#include <QGuiApplication>
#include <QtTest>

#include "bridge_android.h"

class BridgeAndroidTest : public QObject {
  Q_OBJECT

private Q_SLOTS:
  void directories_are_siblings_in_the_app_files() {
    const QString data = AndroidBridge::dataDir();
    const QString runtime = AndroidBridge::runtimeDir();
    QVERIFY(data.startsWith(QLatin1Char('/')));
    QCOMPARE(QFileInfo(data).fileName(), QStringLiteral("eco"));
    QCOMPARE(QFileInfo(runtime).fileName(), QStringLiteral("run"));
    QCOMPARE(QFileInfo(data).absolutePath(), QFileInfo(runtime).absolutePath());
  }

  void the_socket_is_in_the_runtime_directory_and_fits_a_unix_address() {
    const QString socket = AndroidBridge::socketPath();
    QCOMPARE(socket, AndroidBridge::runtimeDir() + QStringLiteral("/eco.sock"));
    // sockaddr_un.sun_path holds 108 bytes.
    QVERIFY(socket.toUtf8().size() < 108);
  }

  void start_names_the_socket_for_the_window() {
    qunsetenv("ECO_SOCKET");
    AndroidBridge::start();
    QCOMPARE(qEnvironmentVariable("ECO_SOCKET"), AndroidBridge::socketPath());
  }

  void the_services_report_becomes_the_status_the_window_binds() {
    QSignalSpy changed(&AndroidBridge::Status::instance(), &AndroidBridge::Status::changed);
    AndroidBridge::report(QStringLiteral("models.failed"));
    QTRY_COMPARE(AndroidBridge::Status::instance().value(), QStringLiteral("models.failed"));
    QCOMPARE(changed.count(), 1);
    // The same status again is not a change.
    AndroidBridge::report(QStringLiteral("models.failed"));
    AndroidBridge::report(QString());
    QTRY_COMPARE(AndroidBridge::Status::instance().value(), QString());
    QCOMPARE(changed.count(), 2);
  }
};

int main(int argc, char **argv) {
  QGuiApplication app(argc, argv);
  BridgeAndroidTest test;
  return QTest::qExec(&test, argc, argv);
}

#include "bridge_android_test.moc"
