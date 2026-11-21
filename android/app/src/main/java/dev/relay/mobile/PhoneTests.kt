package dev.relay.mobile

import androidx.compose.foundation.layout.*
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONObject

@Composable
internal fun PhoneTests(vm: RelayViewModel, chat: Chat, close: () -> Unit) {
    val p = LocalPalette.current
    var paired by remember(vm.current?.id) { mutableStateOf(vm.phoneKey().isNotEmpty()) }
    var input by remember { mutableStateOf("") }
    var error by remember { mutableStateOf("") }
    Sheet(close) {
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Label("Тесты на ПК", 18.sp, p.text, FontWeight.SemiBold)
            Label(if (chat.phoneOnline) "Фоновый Relay на связи" else "Нет связи с фоновым Relay", 13.sp, p.muted)
            if (!paired) {
                Label("Введи код сопряжения с компьютера.", 14.sp, p.text)
                Field(input, { input = it.trim().lowercase() }, "Код сопряжения", password = true)
                PrimaryButton("Связать с ПК", input.isNotBlank()) {
                    paired = vm.pairPhone(input)
                    if (paired) input = "" else error = "Проверь код из 32 знаков и хранилище телефона."
                }
                if (error.isNotEmpty()) Label(error, 13.sp, p.red)
            } else {
                Label("Попроси агента запустить тест в чате. Здесь появятся запрос на запуск и результат.", 13.sp, p.muted)
                for (job in chat.phoneJobs) key(job.optString("id")) {
                    val payload = runCatching { JSONObject(job.optString("payload")) }.getOrNull()
                    val args = payload?.optJSONObject("input")
                    val result = job.optJSONObject("result")
                    val stage = job.optString("stage")
                    val command = job.optString("tool") == "run_local_command"
                    androidx.compose.material3.HorizontalDivider(color = p.line)
                    Label(if (command) "Команда на ПК" else "Игровой тест", 15.sp, p.text, FontWeight.SemiBold)
                    if (stage == "approval" && args != null) {
                        Text(if (command) buildString {
                            append(args.optString("command"))
                            val files = args.optJSONArray("preview_files")
                            for (i in 0 until (files?.length() ?: 0)) append("\nПревью: " + files?.optString(i))
                        } else buildString {
                            appendLine(args.optString("purpose")); appendLine(args.optString("scenario"))
                            val mods = args.optJSONArray("mods")
                            for (i in 0 until (mods?.length() ?: 0)) appendLine(mods?.optJSONObject(i)?.optString("server_path"))
                        }, color = p.text, fontSize = 14.sp)
                        PrimaryButton("Запустить на ПК") { vm.decidePhone(chat, job, true) }
                        GhostButton("Отклонить") { vm.decidePhone(chat, job, false) }
                    } else if (stage == "done" && result != null) {
                        Label(if (result.optBoolean("ok")) "Успешно" else "Завершён с ошибкой", 14.sp, p.text)
                        val output = listOf("message", "stdout", "stderr").map { result.optString(it) }.filter { it.isNotBlank() }.joinToString("\n")
                        if (output.isNotEmpty()) Text(output.takeLast(12000), color = p.text, fontSize = 13.sp)
                        val files = result.optJSONArray("files")
                        for (i in 0 until (files?.length() ?: 0)) {
                            val path = files?.optString(i).orEmpty()
                            val transfer = vm.transfers.firstOrNull { it.path == path }
                            ServerImagePreview(vm, chat, path)
                            GhostButton(path.substringAfterLast('/'), p.accent) {
                                if (transfer?.done == true) vm.openTransfer(transfer) else vm.download(path, chat.cwd)
                            }
                        }
                    } else {
                        Label(job.optString("message").ifBlank { "Подключение к ПК…" }, 13.sp, p.muted)
                        if (stage == "running") GhostButton("Остановить") { vm.stopPhone(chat, job.optString("id")) }
                    }
                }
                GhostButton("Отвязать этот телефон") { if (vm.pairPhone("")) paired = false }
            }
        }
    }
}
