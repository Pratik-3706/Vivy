package com.vivy.agent.model

import kotlinx.serialization.Serializable

// NOTE: every outgoing message carries its "type" as a default value. The Json
// instance in WebSocketManager must use encodeDefaults = true, otherwise "type"
// is dropped and the PC can't parse anything the phone sends.

@Serializable
data class PhoneCommand(
    val id: String = "",
    val type: String,
    val x: Float? = null,
    val y: Float? = null,
    val x1: Float? = null,
    val y1: Float? = null,
    val x2: Float? = null,
    val y2: Float? = null,
    val duration_ms: Long? = null,
    val text: String? = null,
    val query: String? = null,
    val key: String? = null,
    val package_name: String? = null,
    val app_name: String? = null,
    val direction: String? = null,
    val fps: Int? = null,
    val quality: Int? = null,
    val max_dimension: Int? = null
)

@Serializable
data class CommandResult(
    val id: String,
    val type: String = "command_result",
    val success: Boolean,
    val error: String? = null,
    val data: String? = null
)

@Serializable
data class ConnectedMessage(
    val type: String = "connected",
    val device_name: String,
    val brand: String,
    val model: String,
    val android_version: String,
    val screen_width: Int,
    val screen_height: Int,
    val battery_level: Int,
    val is_charging: Boolean,
    val accessibility: Boolean,
    val screen_share: Boolean
)

/** A crash recorded on the phone, sent on the next connection. */
@Serializable
data class CrashReportMessage(
    val type: String = "crash_report",
    val report: String
)

/** Sent whenever accessibility or screen sharing turns on or off. */
@Serializable
data class StatusMessage(
    val type: String = "device_status",
    val accessibility: Boolean,
    val screen_share: Boolean
)

@Serializable
data class ErrorMessage(
    val type: String = "error",
    val message: String
)

@Serializable
data class HeartbeatMessage(
    val type: String = "heartbeat",
    val timestamp: Long
)
