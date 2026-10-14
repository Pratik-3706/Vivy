package com.vivy.agent.util

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.util.DisplayMetrics
import android.view.WindowManager

object Screen {
    /**
     * Full physical display size in the current orientation, including status and
     * navigation bars. Screen capture and gestures must both use this so that a
     * percentage on the PC maps to the same pixel on the phone.
     */
    fun realSize(context: Context): Pair<Int, Int> {
        val wm = context.getSystemService(Context.WINDOW_SERVICE) as WindowManager
        val metrics = DisplayMetrics()
        @Suppress("DEPRECATION")
        wm.defaultDisplay.getRealMetrics(metrics)
        return metrics.widthPixels to metrics.heightPixels
    }

    fun densityDpi(context: Context): Int = context.resources.displayMetrics.densityDpi

    @Suppress("DEPRECATION")
    fun rotation(context: Context): Int =
        (context.getSystemService(Context.WINDOW_SERVICE) as WindowManager).defaultDisplay.rotation
}

object AppLauncher {

    data class Match(val intent: Intent, val label: String, val packageName: String)

    /**
     * Resolve an app from a package id and/or a human name. Package ids are tried
     * first; otherwise launcher apps are matched by label (exact, prefix, then
     * substring) so "youtube" finds YouTube without the model knowing the package.
     */
    fun resolve(context: Context, packageName: String?, appName: String?): Match? {
        val pm = context.packageManager

        if (!packageName.isNullOrBlank()) {
            pm.getLaunchIntentForPackage(packageName.trim())?.let {
                return Match(it, labelOf(pm, packageName.trim()), packageName.trim())
            }
        }

        val query = (appName ?: packageName)?.trim()?.lowercase().orEmpty()
        if (query.isEmpty()) return null

        val launcher = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        val candidates = pm.queryIntentActivities(launcher, 0).map { ri ->
            Triple(ri, ri.loadLabel(pm).toString(), ri.activityInfo.packageName)
        }

        fun score(label: String, pkg: String): Int {
            val l = label.lowercase()
            return when {
                l == query -> 100
                pkg.lowercase() == query -> 95
                l.startsWith(query) -> 80
                l.split(' ').any { it == query } -> 70
                l.contains(query) -> 50
                pkg.lowercase().contains(query.replace(" ", "")) -> 30
                else -> 0
            }
        }

        val best = candidates
            .map { (ri, label, pkg) -> Triple(ri, label, score(label, pkg)) }
            .filter { it.third > 0 }
            .maxWithOrNull(compareBy<Triple<android.content.pm.ResolveInfo, String, Int>> { it.third }
                .thenBy { -it.second.length })
            ?: return null

        val ri = best.first
        val intent = Intent(Intent.ACTION_MAIN)
            .addCategory(Intent.CATEGORY_LAUNCHER)
            .setClassName(ri.activityInfo.packageName, ri.activityInfo.name)
        return Match(intent, best.second, ri.activityInfo.packageName)
    }

    private fun labelOf(pm: PackageManager, pkg: String): String = try {
        pm.getApplicationLabel(pm.getApplicationInfo(pkg, 0)).toString()
    } catch (e: Exception) {
        pkg
    }
}
