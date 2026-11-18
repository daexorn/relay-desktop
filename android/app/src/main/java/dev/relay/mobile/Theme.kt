package dev.relay.mobile

import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.lerp

/** Shared 24-theme palette synced with Desktop & TUI (`src/model.rs` THEME_SPECS): base, side, rail, surface, line, text, muted, accent. */
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
    val green = if (light) Color(0xFF23865B) else Color(0xFF5CB884)
    val red = if (light) Color(0xFFC94A55) else Color(0xFFDA747B)
    val amber = if (light) Color(0xFFB87424) else Color(0xFFE5B567)
    val elevated get() = lerp(surface, if (light) base else text, if (light) 0.35f else 0.045f)
    val subtleAccent get() = accent.copy(alpha = if (light) 0.12f else 0.15f)
    val borderSubtle get() = line.copy(alpha = if (light) 0.75f else 0.65f)
    val specularTop get() = lerp(line, if (light) Color.White else text, if (light) 0.55f else 0.22f)
    fun mix(a: Color, b: Color, t: Float) = lerp(a, b, t)
    fun phaseColor(phase: ToolPhase): Color = phaseColor(phase, light)
}

/** Distinct engine signature colors across Dark & Light palettes. */
fun backendColor(backend: String, light: Boolean): Color = when (backend) {
    "claude" -> if (light) Color(0xFFCA6234) else Color(0xFFEC8A5C)
    "codex" -> if (light) Color(0xFF169873) else Color(0xFF3ED6A8)
    "agy" -> if (light) Color(0xFF6E4AE0) else Color(0xFFA482FF)
    "api" -> if (light) Color(0xFF256CDA) else Color(0xFF5EA8FF)
    else -> if (light) Color(0xFFCA6234) else Color(0xFFEC8A5C)
}

fun backendColor(p: Palette, backend: String): Color = backendColor(backend, p.light)

/** Distinct telemetry phase colors for pipeline bars, timeline nodes, and badges. */
fun phaseColor(phase: ToolPhase, light: Boolean): Color = when (phase) {
    ToolPhase.Research -> if (light) Color(0xFF1F74C9) else Color(0xFF63B3FF)
    ToolPhase.Edit -> if (light) Color(0xFF1A8C5B) else Color(0xFF4AD696)
    ToolPhase.Terminal -> if (light) Color(0xFFB97118) else Color(0xFFF0B45B)
    ToolPhase.Subagent -> if (light) Color(0xFF6E4AE0) else Color(0xFFA482FF)
    ToolPhase.Desktop -> if (light) Color(0xFF14828E) else Color(0xFF46D6E2)
    ToolPhase.Other -> if (light) Color(0xFF5A6A82) else Color(0xFF93A6C2)
}

data class ThemeInfo(val id: String, val name: String, val light: Boolean, val colors: LongArray)

val THEMES = listOf(
    // 30 Dark Themes
    ThemeInfo("graphite", "Графит", false, longArrayOf(0x10141a, 0x151b23, 0x0c1016, 0x1b242e, 0x2a3643, 0xe6edf3, 0x8d9cab, 0x83c4b3)),
    ThemeInfo("midnight", "Полночь", false, longArrayOf(0x0f1520, 0x141d2b, 0x0b111b, 0x1b2739, 0x2b3b52, 0xe5edf9, 0x95a5bd, 0x89aef1)),
    ThemeInfo("obsidian", "Обсидиан", false, longArrayOf(0x0d0f12, 0x12151a, 0x090b0e, 0x191d24, 0x2a303b, 0xedeff2, 0x9098a6, 0xe5b567)),
    ThemeInfo("tokyo", "Токио", false, longArrayOf(0x1a1b26, 0x16161e, 0x111219, 0x24283b, 0x343b58, 0xc0caf5, 0x7982a9, 0x7aa2f7)),
    ThemeInfo("catppuccin", "Мокко", false, longArrayOf(0x1e1e2e, 0x181825, 0x11111b, 0x313244, 0x45475a, 0xcdd6f4, 0x9399b2, 0xb4befe)),
    ThemeInfo("gruvbox", "Грувбокс", false, longArrayOf(0x1d2021, 0x282828, 0x161819, 0x32302f, 0x504945, 0xebdbb2, 0xa89984, 0xfabd2f)),
    ThemeInfo("kanagawa", "Канагава", false, longArrayOf(0x1f1f28, 0x181820, 0x121218, 0x2a2a37, 0x3a3a4e, 0xdcd7ba, 0x938aa9, 0xdca561)),
    ThemeInfo("everforest", "Тайга", false, longArrayOf(0x232a2e, 0x1e2428, 0x181d20, 0x2d353b, 0x404c51, 0xd3c6aa, 0x859289, 0xa7c080)),
    ThemeInfo("cyber", "Неон", false, longArrayOf(0x0b0e17, 0x101422, 0x070910, 0x171d30, 0x273252, 0xe6f1ff, 0x8496b8, 0x38e8ff)),
    ThemeInfo("aurora", "Аврора", false, longArrayOf(0x0d171c, 0x111f26, 0x091115, 0x182b34, 0x26424f, 0xe4f5f4, 0x88aeb0, 0x5eead4)),
    ThemeInfo("forest", "Хвоя", false, longArrayOf(0x111916, 0x17211c, 0x0c120f, 0x202e27, 0x2e4136, 0xe4eee8, 0x9aad9f, 0x83c6a9)),
    ThemeInfo("plum", "Слива", false, longArrayOf(0x1b1721, 0x16121c, 0x120e18, 0x292331, 0x3b3246, 0xeee7f5, 0xab9bb8, 0xc39bff)),
    ThemeInfo("ember", "Угли", false, longArrayOf(0x1d1816, 0x171311, 0x120f0d, 0x2a2320, 0x3d322d, 0xf3e9e4, 0xb0a097, 0xff8a5b)),
    ThemeInfo("ocean", "Океан", false, longArrayOf(0x0f1b1e, 0x0c1619, 0x091214, 0x172a2e, 0x24393e, 0xe2f2f2, 0x8fadaf, 0x4fd1c5)),
    ThemeInfo("nord", "Север", false, longArrayOf(0x2e3440, 0x292e39, 0x242933, 0x3b4252, 0x434c5e, 0xeceff4, 0x9aa5b8, 0x88c0d0)),
    ThemeInfo("rose", "Роза", false, longArrayOf(0x1e1619, 0x181114, 0x130d10, 0x2c2024, 0x402e34, 0xf6e8ec, 0xb79aa3, 0xf28ba8)),
    ThemeInfo("dracula", "Дракула", false, longArrayOf(0x282a36, 0x21222c, 0x1b1c24, 0x343746, 0x44475a, 0xf8f8f2, 0xa4a9c6, 0xbd93f9)),
    ThemeInfo("oled", "OLED", false, longArrayOf(0x000000, 0x050506, 0x000000, 0x121214, 0x232327, 0xf2f2f4, 0x8a8a93, 0xa78bfa)),
    ThemeInfo("monokai", "Монокай", false, longArrayOf(0x1e1f1c, 0x181916, 0x131411, 0x272822, 0x3e3d32, 0xf8f8f2, 0xa59f85, 0xa6e22e)),
    ThemeInfo("ayu", "Аю Мираж", false, longArrayOf(0x1f2430, 0x191e2a, 0x141925, 0x272d38, 0x384152, 0xcccac2, 0x8a9199, 0xffcc66)),
    ThemeInfo("synthwave", "Синтвейв", false, longArrayOf(0x1b1429, 0x150f21, 0x100b1a, 0x261c3a, 0x3d2c5e, 0xf9f2ff, 0xa792c9, 0xff7edb)),
    ThemeInfo("cobalt", "Кобальт", false, longArrayOf(0x122238, 0x0e1b2d, 0x0a1422, 0x192e4a, 0x274469, 0xe8f1fc, 0x8fa9c9, 0xffc600)),
    ThemeInfo("matrix", "Матрица", false, longArrayOf(0x080f0a, 0x0c160f, 0x050a06, 0x112116, 0x1d3825, 0xd8f8e1, 0x73a882, 0x39ff88)),
    ThemeInfo("vesper", "Веспер", false, longArrayOf(0x101010, 0x161616, 0x0b0b0b, 0x1e1e1e, 0x2e2e2e, 0xf0f0f0, 0x8c8c8c, 0xffc799)),
    ThemeInfo("poimandres", "Поймандрес", false, longArrayOf(0x1b1e28, 0x161922, 0x12141c, 0x252b3b, 0x333c52, 0xe4f0fb, 0x8695b7, 0x5de4c7)),
    ThemeInfo("horizon", "Горизонт", false, longArrayOf(0x1c1e26, 0x16181f, 0x111319, 0x262934, 0x383c4d, 0xe3e6ee, 0x9598a6, 0xe95678)),
    ThemeInfo("zenburn", "Зенбёрн", false, longArrayOf(0x2b2b2b, 0x242424, 0x1e1e1e, 0x383838, 0x4b4b4b, 0xdcdccc, 0x989888, 0x8cd0d3)),
    ThemeInfo("sakura_night", "Сакура Ночь", false, longArrayOf(0x1a131b, 0x140e15, 0x0f0a10, 0x261c28, 0x3b2b3e, 0xf7eaf8, 0xb394b6, 0xff9ce3)),
    ThemeInfo("arctic_void", "Арктика", false, longArrayOf(0x0c131d, 0x101926, 0x080e16, 0x172436, 0x253852, 0xe8f3ff, 0x8aa4c2, 0x70d6ff)),
    ThemeInfo("copper", "Медь", false, longArrayOf(0x181310, 0x1f1915, 0x120e0b, 0x2a211c, 0x3e3129, 0xf2e7df, 0xa89588, 0xe3965e)),
    // 10 Light Themes
    ThemeInfo("paper", "Бумага", true, longArrayOf(0xfcfcfb, 0xf5f4f2, 0xedece9, 0xffffff, 0xe4e2de, 0x1e1e21, 0x78777e, 0x4a6fe3)),
    ThemeInfo("sand", "Песок", true, longArrayOf(0xfbf7f0, 0xf4eee3, 0xece4d6, 0xfffdf9, 0xe3d8c6, 0x2b241c, 0x857865, 0xc2703d)),
    ThemeInfo("mint", "Мята", true, longArrayOf(0xf6fbf8, 0xedf6f1, 0xe2efe8, 0xffffff, 0xd5e7dc, 0x17261e, 0x6c8478, 0x2f9e77)),
    ThemeInfo("latte", "Латте", true, longArrayOf(0xeff1f5, 0xe6e9ef, 0xdce0e8, 0xffffff, 0xccd0da, 0x2c2f3f, 0x6c6f85, 0x1e66f5)),
    ThemeInfo("solarized", "Солярис", true, longArrayOf(0xfdf6e3, 0xf4ecd8, 0xeee8d5, 0xfffbf0, 0xded6c1, 0x37474f, 0x657b83, 0x2aa198)),
    ThemeInfo("frost", "Иней", true, longArrayOf(0xf4f7fb, 0xeaf0f7, 0xdfe8f2, 0xffffff, 0xcfdbe8, 0x172133, 0x62728a, 0x2563eb)),
    ThemeInfo("lavender", "Лаванда", true, longArrayOf(0xf8f6fc, 0xf0ecf9, 0xe6dff4, 0xffffff, 0xd9cef0, 0x251e35, 0x74688c, 0x7c3aed)),
    ThemeInfo("peach", "Персик", true, longArrayOf(0xfff8f4, 0xf9efe9, 0xf2e4db, 0xffffff, 0xe8d5c8, 0x2e1f18, 0x856d60, 0xe06338)),
    ThemeInfo("sage", "Шалфей", true, longArrayOf(0xf5f7f4, 0xebefe9, 0xe0e6dd, 0xffffff, 0xd1dbd0, 0x1e261e, 0x6c7a6c, 0x3b7a57)),
    ThemeInfo("sky_day", "Небо", true, longArrayOf(0xf2f8fd, 0xe8f1fa, 0xdbe9f6, 0xffffff, 0xc9ddf0, 0x152536, 0x5f7891, 0x0284c7)),
)

fun rgb(n: Long) = Color(0xFF000000 or n)

fun palette(id: String): Palette {
    val t = THEMES.firstOrNull { it.id == id } ?: THEMES[0]
    val c = t.colors
    return Palette(rgb(c[0]), rgb(c[1]), rgb(c[2]), rgb(c[3]), rgb(c[4]), rgb(c[5]), rgb(c[6]), rgb(c[7]), t.light)
}

fun isLight(id: String) = THEMES.firstOrNull { it.id == id }?.light ?: false
