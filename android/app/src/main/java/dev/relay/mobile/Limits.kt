package dev.relay.mobile

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.HorizontalDivider
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import org.json.JSONObject
import kotlin.math.roundToInt

internal data class QuotaWindow(val name: String, val percent: Double?, val resetsAt: Long?)
internal data class AgentQuota(val windows: List<QuotaWindow>, val status: String, val reason: String) {
    companion object {
        fun parse(agent: String, event: JSONObject): AgentQuota {
            val rows = event.optJSONArray("windows")
            val windows = buildList {
                for (i in 0 until (rows?.length() ?: 0)) {
                    val row = rows?.optJSONObject(i) ?: continue
                    val id = row.optString("id")
                    if (agent == "agy" && id != "seven_day" && !id.endsWith("_weekly")) continue
                    val name = row.optString("label").ifBlank {
                        when (id) { "five_hour" -> "5 часов"; "seven_day" -> "Неделя"; else -> id }
                    }
                    val percent = row.optDouble("utilization").takeIf { it.isFinite() && it in 0.0..100.0 }
                    val reset = row.optLong("resets_at").takeIf { it > 0 }
                    val label = if (name.endsWith(" · нед")) name.dropLast(3) + "неделя" else name
                    if (percent != null || reset != null) add(QuotaWindow(label, percent, reset))
                }
            }
            val reason = if (event.optBoolean("available") || event.isNull("message")) "" else event.optString("message").take(200)
            return AgentQuota(windows, event.optString("status"), reason)
        }
    }
}

internal fun quotaName(agent: String) = when (agent) {
    "agy" -> "Antigravity"
    "codex" -> "Codex"
    else -> "Claude"
}

private fun resetLabel(reset: Long, now: Long): String {
    val minutes = ((reset - now + 59) / 60).coerceAtLeast(1)
    return "Сброс через " + when {
        minutes >= 1440 -> "${minutes / 1440} д ${minutes / 60 % 24} ч"
        minutes >= 60 -> "${minutes / 60} ч ${minutes % 60} мин"
        else -> "$minutes мин"
    }
}

@Composable
internal fun LimitsSheet(vm: RelayViewModel, close: () -> Unit) {
    val p = LocalPalette.current
    var now by remember { mutableLongStateOf(System.currentTimeMillis() / 1000) }
    LaunchedEffect(vm.current?.id) {
        vm.requestUsage(refresh = true)
        while (true) { now = System.currentTimeMillis() / 1000; delay(30000) }
    }
    Sheet(close) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
            Column(Modifier.weight(1f)) {
                Label("Лимиты агентов", 21.sp, p.text, FontWeight.SemiBold)
                Label(if (vm.link == Link.Online) "Использовано за период" else "Нет связи · последние данные", 13.sp, p.muted)
            }
            if (vm.link == Link.Online) GhostButton("Обновить", p.accent) { vm.requestUsage(refresh = true) }
            IconButton(Icons.Default.Close, "Закрыть лимиты", p.muted, close)
        }
        if (vm.quotaAgents().isEmpty()) Label("На сервере нет агентов с доступными квотами.", 14.sp, p.muted)
        for (agent in vm.quotaAgents()) {
            Spacer(Modifier.height(16.dp))
            HorizontalDivider(color = p.line)
            Spacer(Modifier.height(12.dp))
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                PixelMascot(agent, 18.dp)
                Label(quotaName(agent), 15.sp, p.text, FontWeight.SemiBold)
            }
            val quota = vm.usage[agent]
            val windows = quota?.windows.orEmpty().filter { it.resetsAt == null || it.resetsAt > now }
            if (windows.isEmpty()) {
                val status = when (quota?.status) {
                    "rejected" -> "Лимит достигнут"
                    "allowed_warning" -> "Близко к лимиту"
                    "allowed" -> "Доступно · процент не указан"
                    else -> quota?.reason?.ifBlank { "Лимиты недоступны" } ?: "Пока нет данных"
                }
                Label(status, 13.sp, p.muted, modifier = Modifier.padding(top = 5.dp))
            }
            for (window in windows) {
                Spacer(Modifier.height(10.dp))
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Label(window.name, 13.sp, p.text, modifier = Modifier.weight(1f))
                    Label(window.percent?.let { "${it.roundToInt()}%" } ?: "—", 14.sp, p.text, FontWeight.SemiBold)
                }
                window.percent?.let { percent ->
                    val tint = when { percent >= 95 -> p.red; percent >= 80 -> p.amber; else -> p.accent }
                    Box(Modifier.padding(top = 4.dp).fillMaxWidth().height(4.dp).clip(RoundedCornerShape(2.dp)).background(p.line)) {
                        Box(Modifier.fillMaxWidth((percent / 100).toFloat()).fillMaxHeight().background(tint))
                    }
                }
                window.resetsAt?.let { Label(resetLabel(it, now), 11.sp, p.muted, modifier = Modifier.padding(top = 3.dp)) }
            }
        }
    }
}

@Composable
internal fun ServerOverview(vm: RelayViewModel, running: Int, limits: () -> Unit, radar: () -> Unit) {
    val p = LocalPalette.current
    val agents = vm.quotaAgents()
    val preferred = vm.current?.effectiveBackend()
    val now = System.currentTimeMillis() / 1000
    val summary = (listOfNotNull(preferred) + agents).distinct().filter { it in agents }.firstNotNullOfOrNull { agent ->
        vm.usage[agent]?.windows?.firstOrNull { it.percent != null && (it.resetsAt == null || it.resetsAt > now) }?.let {
            "${quotaName(agent)} · ${it.name} · ${it.percent!!.roundToInt()}%"
        }
    }
    Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp)
        .clip(RoundedCornerShape(14.dp)).background(p.surface), verticalAlignment = Alignment.CenterVertically) {
        OverviewItem(Modifier.weight(1.15f), Glyph.Pulse, "Лимиты", summary ?: "По агентам", limits)
        Box(Modifier.width(1.dp).height(26.dp).background(p.line))
        val word = when {
            running % 10 == 1 && running % 100 != 11 -> "агент активен"
            running % 10 in 2..4 && running % 100 !in 12..14 -> "агента активны"
            else -> "агентов активны"
        }
        OverviewItem(Modifier.weight(1f), Glyph.Radar, "Радар", if (running > 0) "$running $word" else "Нет активных", radar)
    }
}

@Composable
private fun OverviewItem(modifier: Modifier, glyph: Glyph, title: String, detail: String, click: () -> Unit) {
    val p = LocalPalette.current
    Column(modifier.clickable(onClick = click).padding(horizontal = 12.dp, vertical = 9.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            VectorGlyph(glyph, p.muted, 15.dp)
            Label(title, 14.sp, p.text, FontWeight.SemiBold)
        }
        Label(detail, 11.5.sp, p.muted, maxLines = 1)
    }
}
