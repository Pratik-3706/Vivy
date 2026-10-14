package com.vivy.agent.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// Same palette as the desktop app: warm neutrals, color only for state.
val VivyBg = Color(0xFF111110)
val VivySurface = Color(0xFF1A1A18)
val VivyRaised = Color(0xFF22221F)
val VivyLine = Color(0xFF2C2C28)
val VivyText = Color(0xFFECEBE6)
val VivyTextMuted = Color(0xFFA3A29A)
val VivyTextFaint = Color(0xFF6F6E67)
val VivyOk = Color(0xFF6FCF7F)
val VivyWarn = Color(0xFFE0A43A)
val VivyDanger = Color(0xFFEF5F4F)

private val DarkColorScheme = darkColorScheme(
    primary = VivyText,
    onPrimary = VivyBg,
    secondary = VivyTextMuted,
    background = VivyBg,
    surface = VivySurface,
    onBackground = VivyText,
    onSurface = VivyText,
    outline = VivyLine,
    error = VivyDanger
)

@Composable
fun VivyTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = DarkColorScheme,
        content = content
    )
}
