package eco.app;

import android.app.Activity;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ServiceInfo;
import android.net.LocalSocket;
import android.net.LocalSocketAddress;
import android.os.Build;
import android.os.IBinder;
import android.util.Log;
import java.io.BufferedReader;
import java.io.File;
import java.io.IOException;
import java.io.InputStreamReader;
import java.util.concurrent.TimeUnit;

// DaemonService is the foreground service that owns the eco daemon: it runs
// libeco.so from the native library directory, `setup` and then
// `daemon --headless`, with the directories the window host gives it, and ends
// the daemon with the service.
public class DaemonService extends Service {
  private static final String TAG = "eco-daemon";
  private static final String CHANNEL = "eco";
  private static final String DATA = "data";
  private static final String RUNTIME = "runtime";
  private static final long TERMINATE_MS = 3000;

  private volatile Process child;
  private volatile boolean stopped;
  private Thread worker;

  // start runs the service, which starts the daemon in dataDir and runtimeDir.
  public static void start(Context context, String dataDir, String runtimeDir) {
    Intent intent = new Intent(context, DaemonService.class).putExtra(DATA, dataDir).putExtra(RUNTIME, runtimeDir);
    context.startForegroundService(intent);
  }

  // askNotifications asks for the notification the service shows, where the
  // system makes it a permission; the service runs without it.
  public static void askNotifications(Activity activity) {
    String permission = "android.permission.POST_NOTIFICATIONS";
    if (Build.VERSION.SDK_INT >= 33 && activity.checkSelfPermission(permission) != PackageManager.PERMISSION_GRANTED)
      activity.requestPermissions(new String[] {permission}, 1);
  }

  @Override
  public int onStartCommand(Intent intent, int flags, int startId) {
    NotificationManager notifications = getSystemService(NotificationManager.class);
    notifications.createNotificationChannel(new NotificationChannel(CHANNEL, "eco", NotificationManager.IMPORTANCE_LOW));
    Notification notification =
        new Notification.Builder(this, CHANNEL)
            .setContentTitle("eco")
            .setSmallIcon(android.R.drawable.ic_btn_speak_now)
            .setOngoing(true)
            .build();
    startForeground(1, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE);
    if (intent != null && (worker == null || !worker.isAlive()))
      launch(intent.getStringExtra(DATA), intent.getStringExtra(RUNTIME));
    return START_NOT_STICKY;
  }

  @Override
  public void onDestroy() {
    stopped = true;
    Process running = child;
    if (running == null)
      return;
    running.destroy();
    try {
      if (!running.waitFor(TERMINATE_MS, TimeUnit.MILLISECONDS))
        running.destroyForcibly();
    } catch (InterruptedException e) {
      running.destroyForcibly();
    }
  }

  @Override
  public IBinder onBind(Intent intent) {
    return null;
  }

  // answers tells whether a daemon already listens at the socket.
  private static boolean answers(File socket) {
    try (LocalSocket probe = new LocalSocket()) {
      probe.connect(new LocalSocketAddress(socket.getPath(), LocalSocketAddress.Namespace.FILESYSTEM));
      return true;
    } catch (IOException e) {
      return false;
    }
  }

  private void launch(String dataDir, String runtimeDir) {
    new File(dataDir).mkdirs();
    new File(runtimeDir).mkdirs();
    if (answers(new File(runtimeDir, "eco.sock"))) {
      Log.i(TAG, "a daemon already answers at " + runtimeDir);
      return;
    }
    String program = new File(getApplicationInfo().nativeLibraryDir, "libeco.so").getPath();
    // The daemon needs the models, which `setup` downloads when they are missing.
    worker = new Thread(() -> {
      run(new String[] {program, "setup"}, dataDir, runtimeDir);
      if (!stopped)
        run(new String[] {program, "daemon", "--headless"}, dataDir, runtimeDir);
    });
    worker.start();
  }

  // run runs a command with the app's directories until it ends, its output in the log.
  private void run(String[] command, String dataDir, String runtimeDir) {
    ProcessBuilder builder = new ProcessBuilder(command).redirectErrorStream(true);
    builder.environment().put("ECO_DATA_DIR", dataDir);
    builder.environment().put("ECO_RUNTIME_DIR", runtimeDir);
    try {
      child = builder.start();
      Log.i(TAG, command[1] + " started");
      try (BufferedReader out = new BufferedReader(new InputStreamReader(child.getInputStream()))) {
        for (String line; (line = out.readLine()) != null; )
          Log.i(TAG, line);
      }
      Log.i(TAG, command[1] + " ended with " + child.waitFor());
    } catch (IOException | InterruptedException e) {
      Log.e(TAG, command[1] + " did not run", e);
    }
  }
}
