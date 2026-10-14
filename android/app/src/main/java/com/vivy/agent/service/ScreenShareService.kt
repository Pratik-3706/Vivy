package com.vivy.agent.service

import android.app.Activity
import android.app.Notification
import android.app.PendingIntent
import com.vivy.agent.MainActivity
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.PixelFormat
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.ImageReader
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.Log
import androidx.core.app.ServiceCompat
import com.vivy.agent.network.WebSocketManager
import com.vivy.agent.util.Screen
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer

/**
 * Mirrors the screen to the PC. Frames are JPEG-encoded in memory and sent over
 * the WebSocket; nothing is ever written to disk.
 *
 * Android 14+ allows exactly one createVirtualDisplay() per MediaProjection
 * token, so the display is created once and later resized (rotation, quality
 * changes) instead of being recreated.
 */
class ScreenShareService : Service() {

    companion object {
        const val ACTION_START_PROJECTION = "com.vivy.agent.action.START_PROJECTION"
        const val ACTION_STOP_PROJECTION = "com.vivy.agent.action.STOP_PROJECTION"
        const val EXTRA_RESULT_CODE = "extra_result_code"
        const val EXTRA_RESULT_DATA = "extra_result_data"
        const val NOTIFICATION_ID = 1001
        const val CHANNEL_ID = "vivy_screen_share"
        private const val TAG = "VivyScreenShare"

        /** While no PC is connected we still refresh the cached frame, just rarely. */
        private const val IDLE_INTERVAL_MS = 1000L

        @Volatile
        var instance: ScreenShareService? = null
            private set

        private val _isStreaming = MutableStateFlow(false)
        val isStreaming: StateFlow<Boolean> = _isStreaming.asStateFlow()

        private val _hasProjection = MutableStateFlow(false)
        val hasProjection: StateFlow<Boolean> = _hasProjection.asStateFlow()
    }

    // A crash here takes the whole app down, and Android then marks the
    // accessibility service "not working". Log instead.
    private val scope = CoroutineScope(
        SupervisorJob() + Dispatchers.Default +
            CoroutineExceptionHandler { _, e -> Log.e(TAG, "Capture error", e) }
    )

    // Reused for every frame. Allocating fresh full-screen bitmaps 12 times a
    // second churned hundreds of MB/s and could run the app out of memory.
    private var paddedBitmap: Bitmap? = null
    private var frameBitmap: Bitmap? = null
    private val frameOut = ByteArrayOutputStream(256 * 1024)
    private var mediaProjection: MediaProjection? = null
    private var virtualDisplay: VirtualDisplay? = null
    private var imageReader: ImageReader? = null
    private var captureJob: Job? = null

    @Volatile private var lastFrame: ByteArray? = null
    @Volatile private var paused = false
    @Volatile private var targetFps = 12
    @Volatile private var quality = 65
    @Volatile private var maxDimension = 1280
    @Volatile private var needsResize = false

    private var width = 0
    private var height = 0
    private var rotation = -1

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        instance = this
        when (intent?.action) {
            ACTION_START_PROJECTION -> {
                val resultCode = intent.getIntExtra(EXTRA_RESULT_CODE, Activity.RESULT_CANCELED)
                val resultData = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent.getParcelableExtra(EXTRA_RESULT_DATA, Intent::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableExtra(EXTRA_RESULT_DATA)
                }
                if (resultCode == Activity.RESULT_OK && resultData != null) {
                    startProjection(resultCode, resultData)
                } else {
                    stopSelf()
                }
            }
            ACTION_STOP_PROJECTION -> stopScreenShare()
        }
        // A projection token can't survive a restart, so don't ask to be restarted.
        return START_NOT_STICKY
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(CHANNEL_ID, "Screen sharing", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Shown while your screen is mirrored to the PC"
            }
            getSystemService(NotificationManager::class.java)?.createNotificationChannel(channel)
        }
    }

    private fun createNotification(): Notification {
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }
        val immutable = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) PendingIntent.FLAG_IMMUTABLE else 0
        val stopIntent = PendingIntent.getService(
            this, 1,
            Intent(this, ScreenShareService::class.java).setAction(ACTION_STOP_PROJECTION),
            immutable or PendingIntent.FLAG_UPDATE_CURRENT
        )
        val openIntent = PendingIntent.getActivity(
            this, 2,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            immutable or PendingIntent.FLAG_UPDATE_CURRENT
        )
        @Suppress("DEPRECATION")
        return builder
            .setContentTitle("Sharing screen with your PC")
            .setContentText("Live only. Nothing is recorded.")
            .setSmallIcon(android.R.drawable.ic_menu_view)
            .setOngoing(true)
            .setContentIntent(openIntent)
            .addAction(android.R.drawable.ic_menu_close_clear_cancel, "Stop sharing", stopIntent)
            .build()
    }

    /** Swiping Vivy away from recent apps ends sharing and the PC connection. */
    override fun onTaskRemoved(rootIntent: Intent?) {
        WebSocketManager.disconnect()
        stopScreenShare()
        super.onTaskRemoved(rootIntent)
    }

    private fun startProjection(resultCode: Int, resultData: Intent) {
        try {
            // The foreground service (type mediaProjection) must be running before getMediaProjection.
            val notification = createNotification()
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                ServiceCompat.startForeground(
                    this, NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION
                )
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }

            // A fresh grant replaces any old projection.
            releaseCapture(stopProjection = true)

            val manager = getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
            val projection = manager.getMediaProjection(resultCode, resultData) ?: run {
                Log.e(TAG, "getMediaProjection returned null")
                stopScreenShare()
                return
            }
            mediaProjection = projection

            // Required on Android 14+ before createVirtualDisplay.
            projection.registerCallback(object : MediaProjection.Callback() {
                override fun onStop() {
                    // Ignore the old projection's stop signal after a fresh grant;
                    // it used to arrive late and kill the new session.
                    if (mediaProjection !== projection) return
                    WebSocketManager.sendError("Screen sharing was stopped on the phone.")
                    stopScreenShare()
                }
            }, Handler(Looper.getMainLooper()))

            createDisplay(projection)
            _hasProjection.value = true
            startCaptureLoop()
        } catch (e: Exception) {
            Log.e(TAG, "Couldn't start screen share", e)
            stopScreenShare()
        }
    }

    private fun computeSize(): Pair<Int, Int> {
        val (realW, realH) = Screen.realSize(this)
        val longest = maxOf(realW, realH)
        val scale = if (longest > maxDimension) maxDimension.toFloat() / longest else 1f
        // Even dimensions keep hardware encoders and buffers happy.
        val w = ((realW * scale).toInt() / 2) * 2
        val h = ((realH * scale).toInt() / 2) * 2
        return w to h
    }

    private fun createDisplay(projection: MediaProjection) {
        val (w, h) = computeSize()
        width = w
        height = h
        rotation = Screen.rotation(this)
        val reader = ImageReader.newInstance(w, h, PixelFormat.RGBA_8888, 2)
        imageReader = reader
        virtualDisplay = projection.createVirtualDisplay(
            "VivyScreenMirror", w, h, Screen.densityDpi(this),
            DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
            reader.surface, null, null
        )
    }

    /** Swap in a new ImageReader at the current size without recreating the display. */
    private fun resizeDisplay() {
        val vd = virtualDisplay ?: return
        val (w, h) = computeSize()
        rotation = Screen.rotation(this)
        if (w == width && h == height) return
        val newReader = ImageReader.newInstance(w, h, PixelFormat.RGBA_8888, 2)
        vd.resize(w, h, Screen.densityDpi(this))
        vd.surface = newReader.surface
        imageReader?.close()
        imageReader = newReader
        width = w
        height = h
    }

    private fun startCaptureLoop() {
        captureJob?.cancel()
        captureJob = scope.launch {
            var cachedBuffer: ByteBuffer? = null
            var lastEncode = 0L

            while (isActive) {
                val loopStart = System.currentTimeMillis()
                val live = WebSocketManager.isConnected && !paused
                _isStreaming.value = live
                val interval = if (live) 1000L / targetFps.coerceIn(2, 30) else IDLE_INTERVAL_MS

                try {
                    if (needsResize || Screen.rotation(this@ScreenShareService) != rotation) {
                        needsResize = false
                        resizeDisplay()
                    }

                    // Only pull an image when we're ready to encode one. Unread images stay
                    // queued (newest replaces oldest), so the final frame of an animation is
                    // never skipped and a static screen still ends up mirrored correctly.
                    val image = if (loopStart - lastEncode >= interval) imageReader?.acquireLatestImage() else null
                    if (image != null) {
                        try {
                            val plane = image.planes[0]
                            val pixelStride = plane.pixelStride
                            val rowStride = plane.rowStride
                            val w = image.width
                            val h = image.height

                            val needed = rowStride * h
                            if (cachedBuffer == null || cachedBuffer!!.capacity() < needed) {
                                cachedBuffer = ByteBuffer.allocateDirect(needed)
                            }
                            val buf = cachedBuffer!!
                            buf.clear()
                            plane.buffer.rewind()
                            buf.put(plane.buffer)
                            buf.rewind()

                            val paddedWidth = rowStride / pixelStride
                            val padded = paddedBitmap
                                ?.takeIf { it.width == paddedWidth && it.height == h }
                                ?: Bitmap.createBitmap(paddedWidth, h, Bitmap.Config.ARGB_8888).also {
                                    paddedBitmap?.recycle()
                                    paddedBitmap = it
                                }
                            padded.copyPixelsFromBuffer(buf)

                            // Row padding: copy the visible area into a second reused bitmap.
                            val bitmap = if (paddedWidth != w) {
                                val target = frameBitmap
                                    ?.takeIf { it.width == w && it.height == h }
                                    ?: Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888).also {
                                        frameBitmap?.recycle()
                                        frameBitmap = it
                                    }
                                Canvas(target).drawBitmap(padded, 0f, 0f, null)
                                target
                            } else padded

                            frameOut.reset()
                            bitmap.compress(Bitmap.CompressFormat.JPEG, quality.coerceIn(30, 90), frameOut)

                            val jpeg = frameOut.toByteArray()
                            lastFrame = jpeg
                            lastEncode = loopStart
                            if (live) WebSocketManager.sendFrame(jpeg)
                        } finally {
                            image.close()
                        }
                    }
                } catch (e: Throwable) {
                    if (e is CancellationException) throw e
                    Log.e(TAG, "Frame error: ${e.message}", e)
                    if (e is OutOfMemoryError) {
                        // Drop the cached bitmaps and slow down briefly so memory can recover.
                        paddedBitmap?.recycle(); paddedBitmap = null
                        frameBitmap?.recycle(); frameBitmap = null
                        delay(500)
                    }
                }

                val elapsed = System.currentTimeMillis() - loopStart
                delay(maxOf(8L, minOf(interval, 100L) - elapsed))
            }
        }
    }

    fun hasActiveProjection(): Boolean = mediaProjection != null && virtualDisplay != null

    /** Called when a PC connects: resume streaming and send the last frame right away. */
    fun onPeerConnected() {
        paused = false
        sendLatestFrame()
    }

    fun configure(fps: Int, quality: Int, maxDimension: Int) {
        this.targetFps = fps.coerceIn(2, 30)
        this.quality = quality.coerceIn(30, 90)
        if (maxDimension != this.maxDimension) {
            this.maxDimension = maxDimension.coerceIn(480, 2400)
            needsResize = true
        }
    }

    fun setPaused(value: Boolean) {
        paused = value
    }

    fun sendLatestFrame(): Boolean {
        val frame = lastFrame ?: return false
        WebSocketManager.sendFrame(frame)
        return true
    }

    private fun releaseCapture(stopProjection: Boolean) {
        captureJob?.cancel()
        captureJob = null
        paddedBitmap = null
        frameBitmap = null
        try {
            virtualDisplay?.release()
        } catch (_: Exception) {}
        virtualDisplay = null
        try {
            imageReader?.close()
        } catch (_: Exception) {}
        imageReader = null
        if (stopProjection) {
            try {
                mediaProjection?.stop()
            } catch (_: Exception) {}
            mediaProjection = null
        }
        width = 0
        height = 0
    }

    fun stopScreenShare() {
        releaseCapture(stopProjection = true)
        lastFrame = null
        _hasProjection.value = false
        _isStreaming.value = false

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            stopForeground(STOP_FOREGROUND_REMOVE)
        } else {
            @Suppress("DEPRECATION")
            stopForeground(true)
        }
        stopSelf()
    }

    override fun onDestroy() {
        releaseCapture(stopProjection = true)
        _hasProjection.value = false
        _isStreaming.value = false
        scope.cancel()
        if (instance == this) instance = null
        super.onDestroy()
    }
}
