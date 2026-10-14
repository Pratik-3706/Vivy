package com.vivy.agent

import android.app.Application
import android.os.Build
import java.io.File
import java.io.PrintWriter
import java.io.StringWriter
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Records any crash to a file so the next PC connection can report what
 * happened (the accessibility service shows "not working" after a crash).
 */
class VivyApp : Application() {

    companion object {
        private const val CRASH_FILE = "last_crash.txt"

        /** Returns and clears the saved crash report, if any. */
        fun takeCrashReport(app: Application): String? {
            val f = File(app.filesDir, CRASH_FILE)
            if (!f.exists()) return null
            return try {
                f.readText().also { f.delete() }
            } catch (e: Exception) {
                null
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            try {
                val trace = StringWriter().also { error.printStackTrace(PrintWriter(it)) }.toString()
                val version = try {
                    packageManager.getPackageInfo(packageName, 0).versionName
                } catch (e: Exception) {
                    "?"
                }
                val stamp = SimpleDateFormat("yyyy-MM-dd HH:mm:ss", Locale.US).format(Date())
                File(filesDir, CRASH_FILE).writeText(
                    "Crash at $stamp, app $version, Android ${Build.VERSION.RELEASE} (${Build.MANUFACTURER} ${Build.MODEL}), thread ${thread.name}\n$trace"
                )
            } catch (_: Throwable) {
                // never let the recorder itself fail the crash path
            }
            previous?.uncaughtException(thread, error)
        }
    }
}
