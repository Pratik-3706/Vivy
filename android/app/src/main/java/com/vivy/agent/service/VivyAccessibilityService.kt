package com.vivy.agent.service

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.AccessibilityServiceInfo
import android.accessibilityservice.GestureDescription
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Path
import android.graphics.Rect
import android.media.AudioManager
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.text.TextUtils
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityManager
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityWindowInfo
import com.vivy.agent.util.AppLauncher
import com.vivy.agent.util.Screen
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.util.Locale

typealias ResultCallback = (success: Boolean, error: String?, data: String?) -> Unit

class VivyAccessibilityService : AccessibilityService() {

    companion object {
        var instance: VivyAccessibilityService? = null
            private set

        private val _isConnected = MutableStateFlow(false)
        val isConnected: StateFlow<Boolean> = _isConnected.asStateFlow()

        fun isRunning(): Boolean = instance != null

        fun isPermissionGranted(context: Context): Boolean {
            if (isRunning()) return true

            val am = context.getSystemService(Context.ACCESSIBILITY_SERVICE) as? AccessibilityManager
            if (am != null) {
                val enabledServices = am.getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK)
                for (service in enabledServices) {
                    val serviceInfo = service.resolveInfo?.serviceInfo ?: continue
                    if (serviceInfo.packageName == context.packageName &&
                        serviceInfo.name == VivyAccessibilityService::class.java.name
                    ) {
                        return true
                    }
                }
            }

            try {
                val enabledServicesSetting = Settings.Secure.getString(
                    context.contentResolver,
                    Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES
                ) ?: return false

                val colonSplitter = TextUtils.SimpleStringSplitter(':')
                colonSplitter.setString(enabledServicesSetting)
                val expectedFull = "${context.packageName}/${VivyAccessibilityService::class.java.name}"
                val expectedShort = "${context.packageName}/.service.VivyAccessibilityService"

                while (colonSplitter.hasNext()) {
                    val componentNameString = colonSplitter.next()
                    if (componentNameString.equals(expectedFull, ignoreCase = true) ||
                        componentNameString.equals(expectedShort, ignoreCase = true) ||
                        (componentNameString.contains(context.packageName) &&
                         componentNameString.contains("VivyAccessibilityService"))
                    ) {
                        return true
                    }
                }
            } catch (e: Exception) {
                // Ignore and return false
            }

            return false
        }
    }

    override fun onServiceConnected() {
        super.onServiceConnected()
        instance = this
        _isConnected.value = true
    }

    override fun onUnbind(intent: Intent?): Boolean {
        if (instance == this) {
            instance = null
            _isConnected.value = false
        }
        return super.onUnbind(intent)
    }

    override fun onDestroy() {
        super.onDestroy()
        if (instance == this) {
            instance = null
            _isConnected.value = false
        }
    }

    override fun onAccessibilityEvent(event: AccessibilityEvent?) {}

    override fun onInterrupt() {}

    // ---- Gestures -----------------------------------------------------------

    private fun toPixels(xPercent: Float, yPercent: Float): Pair<Float, Float> {
        val (w, h) = Screen.realSize(this)
        val x = (xPercent.coerceIn(0f, 100f) / 100f) * (w - 1)
        val y = (yPercent.coerceIn(0f, 100f) / 100f) * (h - 1)
        return x to y
    }

    private fun dispatch(path: Path, durationMs: Long, what: String, onResult: ResultCallback) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.N) {
            onResult(false, "Gestures need Android 7.0 or newer", null)
            return
        }
        val stroke = GestureDescription.StrokeDescription(path, 0, durationMs)
        val gesture = GestureDescription.Builder().addStroke(stroke).build()
        val accepted = dispatchGesture(gesture, object : GestureResultCallback() {
            override fun onCompleted(gestureDescription: GestureDescription?) = onResult(true, null, null)
            override fun onCancelled(gestureDescription: GestureDescription?) =
                onResult(false, "$what was interrupted (another touch or the screen changed)", null)
        }, null)
        if (!accepted) onResult(false, "$what was rejected by the system", null)
    }

    fun performTap(xPercent: Float, yPercent: Float, onResult: ResultCallback) {
        val (x, y) = toPixels(xPercent, yPercent)
        dispatch(Path().apply { moveTo(x, y) }, 60, "Tap", onResult)
    }

    fun performLongPress(xPercent: Float, yPercent: Float, durationMs: Long, onResult: ResultCallback) {
        val (x, y) = toPixels(xPercent, yPercent)
        dispatch(Path().apply { moveTo(x, y) }, durationMs.coerceIn(400, 3000), "Long press", onResult)
    }

    fun performSwipe(
        x1Percent: Float, y1Percent: Float,
        x2Percent: Float, y2Percent: Float,
        durationMs: Long,
        onResult: ResultCallback
    ) {
        val (sx, sy) = toPixels(x1Percent, y1Percent)
        val (ex, ey) = toPixels(x2Percent, y2Percent)
        val path = Path().apply {
            moveTo(sx, sy)
            lineTo(ex, ey)
        }
        dispatch(path, durationMs.coerceIn(80, 3000), "Swipe", onResult)
    }

    fun performScroll(direction: String, onResult: ResultCallback) {
        when (direction.uppercase(Locale.ROOT)) {
            "UP" -> performSwipe(50f, 30f, 50f, 72f, 380, onResult)
            "LEFT" -> performSwipe(20f, 50f, 80f, 50f, 320, onResult)
            "RIGHT" -> performSwipe(80f, 50f, 20f, 50f, 320, onResult)
            else -> performSwipe(50f, 72f, 50f, 30f, 380, onResult)
        }
    }

    fun performSystemKey(key: String, onResult: ResultCallback) {
        val k = key.uppercase(Locale.ROOT)
        when (k) {
            "ENTER" -> {
                val node = findFocusedInput()
                if (node == null) {
                    onResult(false, "No text field is focused, so there is nothing to submit", null)
                    return
                }
                val ok = Build.VERSION.SDK_INT >= Build.VERSION_CODES.R &&
                        node.performAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_IME_ENTER.id)
                onResult(
                    ok,
                    if (ok) null else "ENTER isn't supported in this field here; tap the send or search button instead",
                    null
                )
                return
            }
            "VOLUME_UP", "VOLUME_DOWN" -> {
                val am = getSystemService(Context.AUDIO_SERVICE) as AudioManager
                val dir = if (k == "VOLUME_UP") AudioManager.ADJUST_RAISE else AudioManager.ADJUST_LOWER
                am.adjustStreamVolume(AudioManager.STREAM_MUSIC, dir, AudioManager.FLAG_SHOW_UI)
                onResult(true, null, null)
                return
            }
        }
        val action = when (k) {
            "BACK" -> GLOBAL_ACTION_BACK
            "HOME" -> GLOBAL_ACTION_HOME
            "RECENTS" -> GLOBAL_ACTION_RECENTS
            "NOTIFICATIONS" -> GLOBAL_ACTION_NOTIFICATIONS
            "QUICK_SETTINGS" -> GLOBAL_ACTION_QUICK_SETTINGS
            else -> {
                onResult(false, "Unknown key $key", null)
                return
            }
        }
        val ok = performGlobalAction(action)
        onResult(ok, if (ok) null else "System refused key $key", null)
    }

    // ---- Text input ---------------------------------------------------------

    private fun findFocusedInput(): AccessibilityNodeInfo? {
        for (root in windowRoots()) {
            root.findFocus(AccessibilityNodeInfo.FOCUS_INPUT)?.let { return it }
        }
        return null
    }

    fun typeText(text: String, onResult: ResultCallback) {
        val node = findFocusedInput()
        if (node == null) {
            onResult(false, "No text field is focused. Tap the input field first.", null)
            return
        }

        val args = Bundle().apply {
            putCharSequence(AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE, text)
        }
        if (node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)) {
            onResult(true, null, null)
            return
        }

        // Some custom fields ignore SET_TEXT; paste via the clipboard instead.
        try {
            val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            clipboard.setPrimaryClip(ClipData.newPlainText("vivy_input", text))
            val pasted = node.performAction(AccessibilityNodeInfo.ACTION_PASTE)
            onResult(pasted, if (pasted) null else "This field doesn't accept typed or pasted text", null)
        } catch (e: Exception) {
            onResult(false, "Typing failed: ${e.message}", null)
        }
    }

    // ---- Screen understanding ----------------------------------------------

    private fun windowRoots(): List<AccessibilityNodeInfo> {
        val roots = mutableListOf<AccessibilityNodeInfo>()
        try {
            // Topmost window first, so dialogs and popups win over what's behind them.
            windows
                ?.filter {
                    it.type != AccessibilityWindowInfo.TYPE_INPUT_METHOD &&
                        it.type != AccessibilityWindowInfo.TYPE_ACCESSIBILITY_OVERLAY
                }
                ?.sortedByDescending { it.layer }
                ?.forEach { w -> w.root?.let { roots += it } }
        } catch (e: Exception) {
            // The window list can throw while it is changing
        }
        if (roots.isEmpty()) rootInActiveWindow?.let { roots += it }
        return roots
    }

    private fun keyboardVisible(): Boolean = try {
        windows?.any { it.type == AccessibilityWindowInfo.TYPE_INPUT_METHOD } == true
    } catch (e: Exception) {
        false
    }

    private fun labelOf(node: AccessibilityNodeInfo): String {
        val parts = mutableListOf<String>()
        node.text?.toString()?.trim()?.takeIf { it.isNotEmpty() }?.let { parts += it }
        node.contentDescription?.toString()?.trim()?.takeIf { it.isNotEmpty() && it !in parts }?.let { parts += it }
        if (parts.isEmpty() && Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            node.hintText?.toString()?.trim()?.takeIf { it.isNotEmpty() }?.let { parts += "hint: $it" }
        }
        return parts.joinToString(" | ").replace('\n', ' ').take(90)
    }

    private class Element(val node: AccessibilityNodeInfo, val label: String, val cx: Float, val cy: Float)

    private fun collectElements(limit: Int = 150): List<Element> {
        val (sw, sh) = Screen.realSize(this)
        val screenArea = sw.toFloat() * sh
        val out = mutableListOf<Element>()
        val rect = Rect()

        fun walk(node: AccessibilityNodeInfo, depth: Int) {
            if (out.size >= limit || depth > 50 || !node.isVisibleToUser) return
            val label = labelOf(node)
            val interactive = node.isClickable || node.isLongClickable || node.isEditable ||
                    node.isScrollable || node.isCheckable
            if (label.isNotEmpty() || interactive) {
                node.getBoundsInScreen(rect)
                val area = rect.width().toFloat() * rect.height()
                // Unlabeled clickable containers usually just wrap labeled children; skip the big ones.
                val keep = rect.width() > 0 && rect.height() > 0 &&
                        (label.isNotEmpty() || node.isEditable || node.isScrollable || area < screenArea * 0.15f)
                if (keep) {
                    val cx = (rect.exactCenterX() / sw * 100f).coerceIn(0f, 100f)
                    val cy = (rect.exactCenterY() / sh * 100f).coerceIn(0f, 100f)
                    out += Element(node, label, cx, cy)
                }
            }
            for (i in 0 until node.childCount) {
                node.getChild(i)?.let { walk(it, depth + 1) }
            }
        }

        windowRoots().forEach { walk(it, 0) }
        return out
    }

    /** Compact, model-friendly listing of what's on screen. */
    fun dumpUi(): String {
        val sb = StringBuilder()
        val pkg = rootInActiveWindow?.packageName?.toString() ?: "unknown"
        sb.append("Foreground app: ").append(pkg).append('\n')
        if (keyboardVisible()) sb.append("Keyboard: open\n")

        val elements = collectElements()
        if (elements.isEmpty()) {
            sb.append("(No readable elements. The app may draw its own UI; use visual inspection.)\n")
        }
        for (e in elements) {
            val n = e.node
            val role = n.className?.toString()?.substringAfterLast('.') ?: "View"
            val flags = buildList {
                if (n.isClickable) add("tap")
                if (n.isEditable) add("input")
                if (n.isScrollable) add("scroll")
                if (n.isCheckable) add(if (n.isChecked) "on" else "off")
                if (n.isFocused) add("focused")
                if (!n.isEnabled) add("disabled")
            }.joinToString(",")
            val label = if (e.label.isEmpty()) "(no label)" else "\"${e.label}\""
            sb.append(
                String.format(
                    Locale.US, "- %s %s%s @ (%.1f, %.1f)\n",
                    label, role, if (flags.isEmpty()) "" else " [$flags]", e.cx, e.cy
                )
            )
        }
        return sb.toString().trimEnd()
    }

    /** Tap the element best matching [query] by text or description. */
    fun tapElement(query: String, onResult: ResultCallback) {
        val q = query.trim().lowercase(Locale.ROOT)
        val elements = collectElements(400).filter { it.label.isNotEmpty() }

        fun score(label: String): Int {
            val l = label.lowercase(Locale.ROOT)
            val parts = l.split(" | ")
            return when {
                parts.any { it == q } -> 100
                parts.any { it.startsWith(q) } -> 70
                l.contains(q) -> 40
                q.contains(l) && l.length >= 3 -> 20
                else -> 0
            }
        }

        val best = elements
            .map { it to score(it.label) }
            .filter { it.second > 0 }
            .maxByOrNull { (e, s) -> s * 10 + (if (e.node.isClickable) 5 else 0) + (if (e.node.isEnabled) 1 else 0) }
            ?.first

        if (best == null) {
            val visible = elements.map { it.label }.distinct().take(20).joinToString(", ") { "\"$it\"" }
            onResult(false, "Nothing on screen matches \"$query\". Visible labels: $visible", null)
            return
        }

        val done = String.format(Locale.US, "Tapped \"%s\" at (%.1f, %.1f)", best.label, best.cx, best.cy)

        // Prefer a real click on the element (or its clickable parent): it works even
        // when something overlaps the element. Fall back to a touch at its center.
        var target: AccessibilityNodeInfo? = best.node
        while (target != null && !target.isClickable) target = target.parent
        if (target != null && target.isEnabled && target.performAction(AccessibilityNodeInfo.ACTION_CLICK)) {
            onResult(true, null, done)
            return
        }
        performTap(best.cx, best.cy) { ok, err, _ -> onResult(ok, err, if (ok) done else null) }
    }

    /** Launching from the accessibility service is exempt from background activity-start limits. */
    fun openApp(packageName: String?, appName: String?, onResult: ResultCallback) {
        val match = AppLauncher.resolve(this, packageName, appName)
        if (match == null) {
            onResult(false, "No installed app matches \"${appName ?: packageName}\"", null)
            return
        }
        try {
            match.intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED)
            startActivity(match.intent)
            onResult(true, null, "Opened ${match.label} (${match.packageName})")
        } catch (e: Exception) {
            onResult(false, "Couldn't open ${match.label}: ${e.message}", null)
        }
    }
}
