package com.vivy.agent.ui

import android.content.Context
import android.content.Intent
import android.provider.Settings
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import com.vivy.agent.network.Pairing
import com.vivy.agent.network.WebSocketManager
import com.vivy.agent.network.WebSocketManager.ConnectionState
import com.vivy.agent.service.ScreenShareService
import com.vivy.agent.service.VivyAccessibilityService
import com.vivy.agent.ui.theme.*

@Composable
fun ConnectScreen(
    onRequestScreenCapture: () -> Unit,
    onStopScreenShare: () -> Unit,
    onDisconnect: () -> Unit
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current
    val prefs = remember { context.getSharedPreferences("vivy_prefs", Context.MODE_PRIVATE) }

    var hostIp by remember { mutableStateOf(prefs.getString("last_host_ip", "") ?: "") }
    var port by remember { mutableStateOf(prefs.getString("last_port", "8765") ?: "8765") }
    var code by remember { mutableStateOf("") }
    remember { Pairing.load(context) }
    val paired by Pairing.paired.collectAsState()

    val connection by WebSocketManager.connectionState.collectAsState()
    val serviceRunning by VivyAccessibilityService.isConnected.collectAsState()
    val hasScreenShare by ScreenShareService.hasProjection.collectAsState()
    val isStreaming by ScreenShareService.isStreaming.collectAsState()

    var accessibilityOn by remember { mutableStateOf(VivyAccessibilityService.isPermissionGranted(context)) }

    // Re-check when the user comes back from Android settings.
    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                accessibilityOn = VivyAccessibilityService.isPermissionGranted(context)
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }
    LaunchedEffect(serviceRunning) { if (serviceRunning) accessibilityOn = true }

    val openAccessibility = { context.startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)) }
    val connected = connection is ConnectionState.Connected

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(VivyBg)
            .windowInsetsPadding(WindowInsets.safeDrawing)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 22.dp, vertical = 28.dp)
    ) {
        Text("Vivy", fontSize = 30.sp, fontWeight = FontWeight.SemiBold, color = VivyText)
        Text(
            "Lets the Vivy app on your PC see and control this phone.",
            fontSize = 14.sp,
            color = VivyTextMuted,
            modifier = Modifier.padding(top = 4.dp)
        )

        StatusLine(connection, isStreaming, hasScreenShare)

        Spacer(Modifier.height(28.dp))

        Step(
            number = 1,
            title = "Accessibility",
            body = "Needed so the PC can tap, type and swipe.",
            done = accessibilityOn,
            doneLabel = if (serviceRunning) "On" else "Granted",
            actionLabel = "Turn on",
            onAction = openAccessibility
        )
        Divider()
        Step(
            number = 2,
            title = "Screen sharing",
            body = "Streams your screen live to the PC. Nothing is recorded or saved.",
            done = hasScreenShare,
            doneLabel = "On",
            actionLabel = "Allow",
            onAction = onRequestScreenCapture,
            onTurnOff = onStopScreenShare
        )
        Divider()

        // Step 3: pair once, then connect
        Row(verticalAlignment = Alignment.Top, modifier = Modifier.padding(vertical = 16.dp)) {
            StepNumber(3, connected)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    if (paired == null) "Pair with your PC" else "Connect to your PC",
                    fontSize = 16.sp,
                    fontWeight = FontWeight.Medium,
                    color = VivyText
                )
                Text(
                    if (paired == null)
                        "Enter the address and pairing code shown in Vivy on your PC. You only do this once."
                    else
                        "Paired with ${paired?.pcName}. Only that PC can connect, and the connection is encrypted.",
                    fontSize = 13.sp,
                    color = VivyTextMuted,
                    lineHeight = 18.sp,
                    modifier = Modifier.padding(top = 2.dp, bottom = 12.dp)
                )

                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Field(
                        value = hostIp,
                        onValueChange = { hostIp = it.trim() },
                        label = "PC address",
                        placeholder = "192.168.1.20",
                        enabled = !connected,
                        modifier = Modifier.weight(1f)
                    )
                    Field(
                        value = port,
                        onValueChange = { port = it.filter(Char::isDigit).take(5) },
                        label = "Port",
                        placeholder = "8765",
                        enabled = !connected,
                        keyboardType = KeyboardType.Number,
                        modifier = Modifier.width(92.dp)
                    )
                }

                if (paired == null) {
                    Spacer(Modifier.height(8.dp))
                    Field(
                        value = code,
                        onValueChange = { code = it.uppercase().filter { c -> c.isLetterOrDigit() || c == '-' }.take(16) },
                        label = "Pairing code",
                        placeholder = "XXXX-XXXX-XXXX",
                        enabled = !connected,
                        keyboardType = KeyboardType.Ascii,
                        modifier = Modifier.fillMaxWidth()
                    )
                }

                (connection as? ConnectionState.Error)?.let { err ->
                    Text(
                        err.message,
                        color = VivyDanger,
                        fontSize = 13.sp,
                        lineHeight = 18.sp,
                        modifier = Modifier.padding(top = 10.dp)
                    )
                }

                Spacer(Modifier.height(14.dp))

                when (connection) {
                    is ConnectionState.Connected -> OutlinedButton(
                        onClick = onDisconnect,
                        modifier = Modifier.fillMaxWidth().height(48.dp),
                        shape = RoundedCornerShape(10.dp),
                        border = BorderStroke(1.dp, VivyLine),
                        colors = ButtonDefaults.outlinedButtonColors(contentColor = VivyText)
                    ) { Text("Disconnect", fontWeight = FontWeight.Medium) }

                    is ConnectionState.Connecting -> Button(
                        onClick = {},
                        enabled = false,
                        modifier = Modifier.fillMaxWidth().height(48.dp),
                        shape = RoundedCornerShape(10.dp)
                    ) {
                        CircularProgressIndicator(Modifier.size(16.dp), color = VivyTextMuted, strokeWidth = 2.dp)
                        Spacer(Modifier.width(10.dp))
                        Text(if (paired == null) "Pairing…" else "Connecting…")
                    }

                    else -> {
                        val needsCode = paired == null && !Pairing.isCompleteCode(code)
                        val ready = accessibilityOn && hasScreenShare && hostIp.isNotBlank() && !needsCode
                        Button(
                            onClick = {
                                when {
                                    !accessibilityOn -> openAccessibility()
                                    !hasScreenShare -> onRequestScreenCapture()
                                    else -> {
                                        prefs.edit()
                                            .putString("last_host_ip", hostIp.trim())
                                            .putString("last_port", port.trim())
                                            .apply()
                                        WebSocketManager.connect(
                                            context,
                                            hostIp.trim(),
                                            port.toIntOrNull() ?: 8765,
                                            code = if (paired == null) code else null
                                        )
                                    }
                                }
                            },
                            enabled = ready || !accessibilityOn || !hasScreenShare,
                            modifier = Modifier.fillMaxWidth().height(48.dp),
                            shape = RoundedCornerShape(10.dp),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = VivyText,
                                contentColor = VivyBg,
                                disabledContainerColor = VivyRaised,
                                disabledContentColor = VivyTextFaint
                            )
                        ) {
                            Text(
                                when {
                                    !accessibilityOn -> "Turn on accessibility first"
                                    !hasScreenShare -> "Allow screen sharing first"
                                    hostIp.isBlank() -> "Enter the PC address"
                                    needsCode -> "Enter the 12-character code"
                                    paired == null -> "Pair"
                                    else -> "Connect"
                                },
                                fontWeight = FontWeight.SemiBold
                            )
                        }
                    }
                }

                if (paired != null && !connected) {
                    Text(
                        "Forget this PC",
                        color = VivyTextMuted,
                        fontSize = 12.sp,
                        modifier = Modifier
                            .padding(top = 12.dp)
                            .clickable {
                                WebSocketManager.disconnect()
                                Pairing.forget(context)
                                code = ""
                            }
                    )
                }
            }
        }

        Spacer(Modifier.height(24.dp))
        Text(
            "Private content (photos, files, messages, email), deletions and payments always need your OK on the PC first.",
            fontSize = 12.sp,
            lineHeight = 17.sp,
            color = VivyTextFaint
        )
    }
}

@Composable
private fun StatusLine(connection: ConnectionState, streaming: Boolean, hasScreenShare: Boolean) {
    val (color, text) = when (connection) {
        is ConnectionState.Connected -> when {
            streaming -> VivyOk to "Connected to ${connection.host} · sharing screen"
            hasScreenShare -> VivyOk to "Connected to ${connection.host}"
            else -> VivyWarn to "Connected to ${connection.host} · screen sharing is off"
        }
        is ConnectionState.Connecting -> VivyWarn to "Connecting…"
        is ConnectionState.Error -> VivyDanger to "Not connected"
        ConnectionState.Disconnected -> VivyTextFaint to "Not connected"
    }
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 18.dp)) {
        Box(Modifier.size(8.dp).background(color, CircleShape))
        Spacer(Modifier.width(8.dp))
        Text(text, fontSize = 13.sp, color = VivyTextMuted)
    }
}

@Composable
private fun Step(
    number: Int,
    title: String,
    body: String,
    done: Boolean,
    doneLabel: String,
    actionLabel: String,
    onAction: () -> Unit,
    onTurnOff: (() -> Unit)? = null
) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.fillMaxWidth().padding(vertical = 16.dp)
    ) {
        StepNumber(number, done)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(title, fontSize = 16.sp, fontWeight = FontWeight.Medium, color = VivyText)
            Text(
                body,
                fontSize = 13.sp,
                lineHeight = 18.sp,
                color = VivyTextMuted,
                modifier = Modifier.padding(top = 2.dp)
            )
        }
        Spacer(Modifier.width(12.dp))
        if (done) {
            Column(horizontalAlignment = Alignment.End) {
                Text(doneLabel, color = VivyOk, fontSize = 13.sp, fontWeight = FontWeight.Medium)
                if (onTurnOff != null) {
                    Text(
                        "Turn off",
                        color = VivyTextMuted,
                        fontSize = 12.sp,
                        modifier = Modifier
                            .padding(top = 4.dp)
                            .clickable(onClick = onTurnOff)
                    )
                }
            }
        } else {
            Button(
                onClick = onAction,
                shape = RoundedCornerShape(8.dp),
                contentPadding = PaddingValues(horizontal = 14.dp, vertical = 6.dp),
                colors = ButtonDefaults.buttonColors(containerColor = VivyText, contentColor = VivyBg)
            ) { Text(actionLabel, fontSize = 13.sp, fontWeight = FontWeight.SemiBold) }
        }
    }
}

@Composable
private fun StepNumber(number: Int, done: Boolean) {
    Box(
        contentAlignment = Alignment.Center,
        modifier = Modifier
            .size(26.dp)
            .border(1.dp, if (done) VivyOk else VivyLine, CircleShape)
    ) {
        Text(
            if (done) "✓" else number.toString(),
            fontSize = 12.sp,
            fontFamily = FontFamily.Monospace,
            color = if (done) VivyOk else VivyTextMuted
        )
    }
}

@Composable
private fun Divider() {
    HorizontalDivider(color = VivyLine, thickness = 1.dp)
}

@Composable
private fun Field(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    placeholder: String,
    enabled: Boolean,
    modifier: Modifier = Modifier,
    keyboardType: KeyboardType = KeyboardType.Uri
) {
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        label = { Text(label) },
        placeholder = { Text(placeholder, color = VivyTextFaint) },
        singleLine = true,
        enabled = enabled,
        keyboardOptions = KeyboardOptions(keyboardType = keyboardType),
        textStyle = LocalTextStyle.current.copy(fontFamily = FontFamily.Monospace, fontSize = 15.sp),
        shape = RoundedCornerShape(10.dp),
        colors = OutlinedTextFieldDefaults.colors(
            focusedBorderColor = VivyTextMuted,
            unfocusedBorderColor = VivyLine,
            disabledBorderColor = VivyLine,
            focusedLabelColor = VivyText,
            unfocusedLabelColor = VivyTextFaint,
            cursorColor = VivyText,
            focusedTextColor = VivyText,
            unfocusedTextColor = VivyText,
            disabledTextColor = VivyTextMuted,
            focusedContainerColor = Color.Transparent,
            unfocusedContainerColor = Color.Transparent
        ),
        modifier = modifier
    )
}
