package com.vivy.agent.network

import android.content.Context
import android.os.BatteryManager
import android.os.Build
import com.vivy.agent.model.*
import com.vivy.agent.service.ScreenShareService
import com.vivy.agent.service.VivyAccessibilityService
import com.vivy.agent.util.Screen
import android.util.Log
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import okhttp3.*
import okio.ByteString.Companion.toByteString
import java.net.ConnectException
import java.net.SocketTimeoutException
import java.net.UnknownHostException
import java.security.cert.CertificateException
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLException

object WebSocketManager {

    // encodeDefaults is essential: each message's "type" is a default value and
    // the PC dispatches on it. Without this the PC silently drops every message.
    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
    }

    private val client = OkHttpClient.Builder()
        .connectTimeout(6, TimeUnit.SECONDS)
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .pingInterval(15, TimeUnit.SECONDS)
        .build()

    // An uncaught exception in a coroutine would crash the whole app, and Android
    // then switches the accessibility service off. Log it instead.
    private val scope = CoroutineScope(
        SupervisorJob() + Dispatchers.Default +
            CoroutineExceptionHandler { _, e -> Log.e("VivyWebSocket", "Unhandled error", e) }
    )
    private var statusJob: Job? = null

    /** Frames are dropped while this much data is still queued, to keep latency low. */
    private const val MAX_QUEUED_BYTES = 1_500_000L

    @Volatile
    private var webSocket: WebSocket? = null
    private lateinit var appContext: Context

    /** Until the PC accepts this phone, nothing but pairing/auth is sent or handled. */
    @Volatile private var ready = false
    private var trust: Pairing.PinnedTrust? = null
    private var pairCode: String? = null
    private var phoneNonce: String = ""

    private val _connectionState = MutableStateFlow<ConnectionState>(ConnectionState.Disconnected)
    val connectionState: StateFlow<ConnectionState> = _connectionState

    val isConnected: Boolean get() = _connectionState.value is ConnectionState.Connected

    sealed class ConnectionState {
        object Disconnected : ConnectionState()
        object Connecting : ConnectionState()
        data class Connected(val host: String) : ConnectionState()
        data class Error(val message: String) : ConnectionState()
    }

    /**
     * Connect to the paired PC, or pair with it first when [code] is given
     * (the code shown in Vivy on the PC). The connection is always encrypted.
     */
    fun connect(context: Context, host: String, port: Int = 8765, code: String? = null) {
        val current = _connectionState.value
        if (current is ConnectionState.Connected || current is ConnectionState.Connecting) return
        if (host.isBlank()) {
            _connectionState.value = ConnectionState.Error("Enter the PC address shown in the Vivy desktop app.")
            return
        }
        appContext = context.applicationContext
        val paired = Pairing.load(appContext)
        if (code == null && paired == null) {
            _connectionState.value = ConnectionState.Error("Pair with your PC first: enter the code shown in Vivy on the PC.")
            return
        }

        _connectionState.value = ConnectionState.Connecting
        ready = false
        pairCode = code
        // Pairing trusts the certificate only after the code proves it; otherwise it must be the pinned one.
        val pinnedTrust = Pairing.PinnedTrust(if (code != null) null else paired?.fingerprint)
        trust = pinnedTrust
        val request = Request.Builder().url("wss://${host.trim()}:$port").build()

        webSocket = Pairing.client(client, pinnedTrust).newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(ws: WebSocket, response: Response) {
                if (ws !== webSocket) return
                if (code != null) {
                    phoneNonce = Pairing.nonce()
                    ws.send(buildJsonObject { put("type", "pair_hello"); put("nonce", phoneNonce) }.toString())
                } else {
                    ws.send(buildJsonObject { put("type", "auth"); put("token", paired!!.token) }.toString())
                }
            }

            override fun onMessage(ws: WebSocket, text: String) {
                if (ws !== webSocket) return
                if (!ready) {
                    handleHandshake(ws, text, host)
                    return
                }
                scope.launch { handleIncomingCommand(ws, text) }
            }

            override fun onClosing(ws: WebSocket, code: Int, reason: String) {
                ws.close(1000, null)
            }

            override fun onClosed(ws: WebSocket, code: Int, reason: String) {
                if (ws !== webSocket) return
                webSocket = null
                ready = false
                statusJob?.cancel()
                _connectionState.value = ConnectionState.Disconnected
            }

            override fun onFailure(ws: WebSocket, t: Throwable, response: Response?) {
                if (ws !== webSocket) return
                webSocket = null
                ready = false
                statusJob?.cancel()
                _connectionState.value = ConnectionState.Error(describe(t, host, port))
            }
        })
    }

    /** Pairing and authentication, before anything else is exchanged. */
    private fun handleHandshake(ws: WebSocket, text: String, host: String) {
        val msg = try {
            json.parseToJsonElement(text).jsonObject
        } catch (e: Exception) {
            return
        }
        fun field(name: String) = msg[name]?.jsonPrimitive?.content ?: ""
        when (field("type")) {
            "pair_challenge" -> {
                val code = pairCode ?: return fail(ws, "Unexpected pairing request.")
                val fp = trust?.seenFingerprint ?: return fail(ws, "Couldn't read the PC's certificate.")
                val pcNonce = field("nonce")
                val expected = Pairing.proof(code, "pc", fp, phoneNonce, pcNonce)
                // A wrong proof means a wrong code, or someone else answering in the PC's place.
                if (!Pairing.sameSecret(expected, field("proof"))) {
                    return fail(ws, "That code didn't match. Check the code shown on the PC (it changes every few minutes) and try again.")
                }
                ws.send(buildJsonObject {
                    put("type", "pair_confirm")
                    put("proof", Pairing.proof(code, "phone", fp, phoneNonce, pcNonce))
                    put("device_name", "${Build.MANUFACTURER} ${Build.MODEL}")
                }.toString())
            }
            "paired" -> {
                val fp = trust?.seenFingerprint ?: return fail(ws, "Couldn't read the PC's certificate.")
                val token = field("token")
                if (token.length < 32) return fail(ws, "The PC sent an invalid pairing reply.")
                Pairing.save(appContext, Pairing.Paired(fp, token, field("pc_name").ifBlank { "PC" }))
                pairCode = null
                onReady(ws, host)
            }
            "auth_ok" -> onReady(ws, host)
            "auth_failed" -> {
                // The PC forgot this phone; the saved pairing is useless now.
                Pairing.forget(appContext)
                fail(ws, field("message").ifBlank { "This phone isn't paired with that PC. Pair again." })
            }
            "pair_error" -> fail(ws, field("message").ifBlank { "Pairing failed." })
        }
    }

    private fun fail(ws: WebSocket, message: String) {
        webSocket = null
        ready = false
        ws.close(1000, null)
        _connectionState.value = ConnectionState.Error(message)
    }

    /** The PC accepted this phone: start the normal session. */
    private fun onReady(ws: WebSocket, host: String) {
        ready = true
        _connectionState.value = ConnectionState.Connected(Pairing.paired.value?.pcName ?: host)
        sendHandshake(ws)
        ScreenShareService.instance?.onPeerConnected()
        if (ScreenShareService.instance?.hasActiveProjection() != true) {
            sendError("Screen sharing is off on the phone. Open Vivy on the phone and allow it.")
        }
        watchStatus(ws)
        (appContext as? android.app.Application)?.let { app ->
            com.vivy.agent.VivyApp.takeCrashReport(app)?.let { report ->
                ws.send(json.encodeToString(CrashReportMessage(report = report.take(20_000))))
            }
        }
    }

    fun disconnect() {
        val ws = webSocket
        webSocket = null
        ready = false
        statusJob?.cancel()
        ws?.close(1000, "User disconnected")
        _connectionState.value = ConnectionState.Disconnected
    }

    /** Tell the PC whenever accessibility or screen sharing turns on or off. */
    private fun watchStatus(ws: WebSocket) {
        statusJob?.cancel()
        statusJob = scope.launch {
            combine(VivyAccessibilityService.isConnected, ScreenShareService.hasProjection) { a, s -> a to s }
                .distinctUntilChanged()
                .collect { (accessibility, screenShare) ->
                    ws.send(json.encodeToString(StatusMessage(accessibility = accessibility, screen_share = screenShare)))
                }
        }
    }

    private fun describe(t: Throwable, host: String, port: Int): String = when {
        t is CertificateException || t.cause is CertificateException ->
            "The PC at $host isn't the one this phone is paired with. If you reinstalled or reset Vivy on the PC, tap \"Forget this PC\" and pair again."
        t is SSLException && pairCode == null ->
            "Couldn't open a secure connection to $host. Update Vivy on the PC, or pair again."
        else -> describePlain(t, host, port)
    }

    private fun describePlain(t: Throwable, host: String, port: Int): String = when (t) {
        is ConnectException -> "Couldn't reach $host:$port. Check that Vivy is open on the PC, both devices are on the same Wi-Fi, and Windows Firewall allows port $port."
        is SocketTimeoutException -> "$host:$port didn't answer in time. The address may be wrong or blocked by a firewall."
        is UnknownHostException -> "\"$host\" isn't a valid address."
        else -> t.message ?: "Connection lost"
    }

    /** Raw JPEG bytes. Binary frames skip base64 + JSON and are about a third smaller. */
    fun sendFrame(jpeg: ByteArray) {
        if (!ready) return
        val ws = webSocket ?: return
        if (ws.queueSize() > MAX_QUEUED_BYTES) return
        ws.send(jpeg.toByteString())
    }

    fun sendError(message: String) {
        if (!ready) return
        webSocket?.send(json.encodeToString(ErrorMessage(message = message)))
    }

    private fun sendHandshake(ws: WebSocket) {
        val (w, h) = Screen.realSize(appContext)

        val bm = appContext.getSystemService(Context.BATTERY_SERVICE) as? BatteryManager
        val batteryPct = bm?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY) ?: -1
        val isCharging = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val status = bm?.getIntProperty(BatteryManager.BATTERY_PROPERTY_STATUS) ?: -1
            status == BatteryManager.BATTERY_STATUS_CHARGING || status == BatteryManager.BATTERY_STATUS_FULL
        } else false

        val msg = ConnectedMessage(
            device_name = "${Build.MANUFACTURER} ${Build.MODEL}",
            brand = Build.BRAND,
            model = Build.MODEL,
            android_version = Build.VERSION.RELEASE,
            screen_width = w,
            screen_height = h,
            battery_level = batteryPct,
            is_charging = isCharging,
            accessibility = VivyAccessibilityService.instance != null,
            screen_share = ScreenShareService.instance?.hasActiveProjection() == true
        )
        ws.send(json.encodeToString(msg))
    }

    private fun handleIncomingCommand(ws: WebSocket, text: String) {
        val cmd = try {
            json.decodeFromString<PhoneCommand>(text)
        } catch (e: Exception) {
            return
        }

        val reply: (Boolean, String?, String?) -> Unit = { success, error, data ->
            ws.send(json.encodeToString(CommandResult(id = cmd.id, success = success, error = error, data = data)))
        }

        // Any failure becomes an error reply. Letting it escape would crash the
        // app, and Android turns the accessibility service off after a crash.
        try {
            dispatch(ws, cmd, reply)
        } catch (e: Throwable) {
            Log.e("VivyWebSocket", "Command ${cmd.type} failed", e)
            reply(false, "Phone error while running ${cmd.type}: ${e.message ?: e.javaClass.simpleName}", null)
        }
    }

    private fun dispatch(ws: WebSocket, cmd: PhoneCommand, reply: (Boolean, String?, String?) -> Unit) {
        // Stream control and device info work without the accessibility service.
        when (cmd.type) {
            "start_stream" -> {
                val share = ScreenShareService.instance
                if (share == null || !share.hasActiveProjection()) {
                    reply(false, "Screen sharing is off on the phone. Allow it in the Vivy app.", null)
                } else {
                    share.configure(cmd.fps ?: 12, cmd.quality ?: 65, cmd.max_dimension ?: 1280)
                    share.onPeerConnected()
                    reply(true, null, null)
                }
                return
            }
            "stop_stream" -> {
                ScreenShareService.instance?.setPaused(true)
                reply(true, null, null)
                return
            }
            "request_frame" -> {
                val sent = ScreenShareService.instance?.sendLatestFrame() ?: false
                reply(sent, if (sent) null else "No frame available yet", null)
                return
            }
            "get_device_info" -> {
                sendHandshake(ws)
                reply(true, null, null)
                return
            }
        }

        // Everything else needs the accessibility service. Without it Android also
        // silently blocks launching apps from the background, so don't pretend.
        val service = VivyAccessibilityService.instance
        if (service == null) {
            reply(
                false,
                "Accessibility for Vivy is turned off on the phone, so it can't tap, type, read the screen or open apps. " +
                    "Turn it on in Settings > Accessibility > Vivy.",
                null
            )
            return
        }

        when (cmd.type) {
            "tap" -> service.performTap(cmd.x ?: 50f, cmd.y ?: 50f, reply)
            "tap_element" -> service.tapElement(cmd.query ?: cmd.text ?: "", reply)
            "long_press" -> service.performLongPress(cmd.x ?: 50f, cmd.y ?: 50f, cmd.duration_ms ?: 800L, reply)
            "swipe" -> service.performSwipe(
                cmd.x1 ?: 50f, cmd.y1 ?: 80f, cmd.x2 ?: 50f, cmd.y2 ?: 20f, cmd.duration_ms ?: 300L, reply
            )
            "scroll" -> service.performScroll(cmd.direction ?: "DOWN", reply)
            "type_text" -> service.typeText(cmd.text ?: "", reply)
            "press_key" -> service.performSystemKey(cmd.key ?: "BACK", reply)
            "open_app" -> service.openApp(cmd.package_name, cmd.app_name, reply)
            "dump_ui" -> reply(true, null, service.dumpUi())
            else -> reply(false, "Unknown command ${cmd.type}", null)
        }
    }
}
