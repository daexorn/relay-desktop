package dev.relay.mobile

import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp

/** Same palette as the desktop app (src/ui.rs THEME_TABLE): base, side, rail, surface, line, text, muted, accent. */
@Immutable
data class Palette(
    val base: Color,
    val side: Color,
    val rail: Color,
    val surface: Color,
    val line: Color,
    val text: Color,
    val muted: Color,
    val accent: Color,
    val light: Boolean,
) {
    val green = Color(0xFF5CB884)
    val red = Color(0xFFDA747B)
    fun mix(a: Color, b: Color, t: Float) = lerp(a, b, t)
}

data class ThemeInfo(val id: String, val name: String, val light: Boolean, val colors: LongArray)

val THEMES = listOf(
    ThemeInfo("graphite", "Графит", false, longArrayOf(0x19191b, 0x131315, 0x0e0e10, 0x222225, 0x2e2e32, 0xececee, 0x8e8e96, 0x8ba6f2)),
    ThemeInfo("midnight", "Полночь", false, longArrayOf(0x111722, 0x0e141f, 0x0b101a, 0x1b2433, 0x2a3547, 0xe5edf9, 0x95a5bd, 0x6ea8ff)),
    ThemeInfo("forest", "Хвоя", false, longArrayOf(0x141c1a, 0x101714, 0x0d1310, 0x202d27, 0x304139, 0xe4eee8, 0x9aad9f, 0x6fcf97)),
    ThemeInfo("plum", "Слива", false, longArrayOf(0x1b1721, 0x16121c, 0x120e18, 0x292331, 0x3b3246, 0xeee7f5, 0xab9bb8, 0xc39bff)),
    ThemeInfo("ember", "Угли", false, longArrayOf(0x1d1816, 0x171311, 0x120f0d, 0x2a2320, 0x3d322d, 0xf3e9e4, 0xb0a097, 0xff8a5b)),
    ThemeInfo("ocean", "Океан", false, longArrayOf(0x0f1b1e, 0x0c1619, 0x091214, 0x172a2e, 0x24393e, 0xe2f2f2, 0x8fadaf, 0x4fd1c5)),
    ThemeInfo("nord", "Север", false, longArrayOf(0x2e3440, 0x292e39, 0x242933, 0x3b4252, 0x434c5e, 0xeceff4, 0x9aa5b8, 0x88c0d0)),
    ThemeInfo("rose", "Роза", false, longArrayOf(0x1e1619, 0x181114, 0x130d10, 0x2c2024, 0x402e34, 0xf6e8ec, 0xb79aa3, 0xf28ba8)),
    ThemeInfo("dracula", "Дракула", false, longArrayOf(0x282a36, 0x21222c, 0x1b1c24, 0x343746, 0x44475a, 0xf8f8f2, 0xa4a9c6, 0xbd93f9)),
    ThemeInfo("oled", "OLED", false, longArrayOf(0x000000, 0x050506, 0x000000, 0x121214, 0x232327, 0xf2f2f4, 0x8a8a93, 0xa78bfa)),
    ThemeInfo("paper", "Бумага", true, longArrayOf(0xfcfcfb, 0xf5f4f2, 0xedece9, 0xffffff, 0xe4e2de, 0x1e1e21, 0x78777e, 0x4a6fe3)),
    ThemeInfo("sand", "Песок", true, longArrayOf(0xfbf7f0, 0xf4eee3, 0xece4d6, 0xfffdf9, 0xe3d8c6, 0x2b241c, 0x857865, 0xc2703d)),
    ThemeInfo("mint", "Мята", true, longArrayOf(0xf6fbf8, 0xedf6f1, 0xe2efe8, 0xffffff, 0xd5e7dc, 0x17261e, 0x6c8478, 0x2f9e77)),
)

fun rgb(n: Long) = Color(0xFF000000 or n)

fun palette(id: String): Palette {
    val t = THEMES.firstOrNull { it.id == id } ?: THEMES[0]
    val c = t.colors
    return Palette(rgb(c[0]), rgb(c[1]), rgb(c[2]), rgb(c[3]), rgb(c[4]), rgb(c[5]), rgb(c[6]), rgb(c[7]), t.light)
}
