package com.vivy.agent

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.media.projection.MediaProjectionManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import com.vivy.agent.network.WebSocketManager
import com.vivy.agent.service.ScreenShareService
import com.vivy.agent.ui.ConnectScreen
import com.vivy.agent.ui.theme.VivyTheme

class MainActivity : ComponentActivity() {

    private val screenCaptureLauncher = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult()
    ) { result ->
        if (result.resultCode == Activity.RESULT_OK && result.data != null) {
            val serviceIntent = Intent(this, ScreenShareService::class.java).apply {
                action = ScreenShareService.ACTION_START_PROJECTION
                putExtra(ScreenShareService.EXTRA_RESULT_CODE, result.resultCode)
                putExtra(ScreenShareService.EXTRA_RESULT_DATA, result.data)
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                ContextCompat.startForegroundService(this, serviceIntent)
            } else {
                startService(serviceIntent)
            }
        }
    }

    private val notificationPermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { /* Permission response handled by system */ }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Request notification permission on Android 13+ for foreground service notification
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            notificationPermissionLauncher.launch(android.Manifest.permission.POST_NOTIFICATIONS)
        }

        setContent {
            VivyTheme {
                ConnectScreen(
                    onRequestScreenCapture = {
                        val mpm = getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
                        screenCaptureLauncher.launch(mpm.createScreenCaptureIntent())
                    },
                    // Keep screen sharing alive so reconnecting doesn't need a new grant.
                    onStopScreenShare = { ScreenShareService.instance?.stopScreenShare() },
                    onDisconnect = { WebSocketManager.disconnect() }
                )
            }
        }
    }
}
