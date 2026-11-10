package dev.relay.mobile

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.material.icons.filled.Close
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.runtime.key
import androidx.compose.material.icons.filled.Share
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.core.MutableTransitionState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale

val LocalPalette = staticCompositionLocalOf { palette("graphite") }

@Composable
fun RelayApp(vm: RelayViewModel) {
    val target = palette(vm.theme)
    // Every colour glides when the theme changes, like on the desktop.
    val p = target.copy(
        base = animateColorAsState(target.base, tween(320), label = "base").value,
        side = animateColorAsState(target.side, tween(320), label = "side").value,
        surface = animateColorAsState(target.surface, tween(320), label = "surface").value,
        line = animateColorAsState(target.line, tween(320), label = "line").value,
        text = animateColorAsState(target.text, tween(320), label = "text").value,
        muted = animateColorAsState(target.muted, tween(320), label = "muted").value,
        accent = animateColorAsState(target.accent, tween(320), label = "accent").value,
    )
    CompositionLocalProvider(LocalPalette provides p) {
        Box(Modifier.fillMaxSize().background(p.base).statusBarsPadding().navigationBarsPadding().imePadding()) {
            AnimatedContent(
                targetState = vm.screen,
                transitionSpec = {
                    val forward = targetState.ordinal > initialState.ordinal
                    (slideInHorizontally(tween(260)) { if (forward) it / 4 else -it / 4 } + fadeIn(tween(260))) togetherWith
                        (slideOutHorizontally(tween(220)) { if (forward) -it / 6 else it / 6 } + fadeOut(tween(180)))
                },
                label = "screen",
            ) { screen ->
                when (screen) {
                    Screen.Servers -> ServersScreen(vm)
                    Screen.Sessions -> SessionsScreen(vm)
                    Screen.Chat -> ChatScreen(vm)
                    Screen.Settings -> SettingsScreen(vm)
                }
            }
            vm.trust?.let { TrustDialog(it, vm) }
        }
    }
}

// --- small building blocks ------------------------------------------------------------

@Composable
fun Label(text: String, size: TextUnit = 14.sp, color: Color = LocalPalette.current.text, weight: FontWeight = FontWeight.Normal, maxLines: Int = Int.MAX_VALUE, modifier: Modifier = Modifier) =
    Text(text, modifier = modifier, color = color, fontSize = size, fontWeight = weight, maxLines = maxLines, overflow = TextOverflow.Ellipsis)

@Composable
fun IconButton(icon: ImageVector, hint: String, tint: Color = LocalPalette.current.muted, onClick: () -> Unit) {
    Box(
        Modifier.size(40.dp).clip(RoundedCornerShape(10.dp)).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) { Icon(icon, contentDescription = hint, tint = tint, modifier = Modifier.size(22.dp)) }
}

@Composable
fun PrimaryButton(text: String, enabled: Boolean = true, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val p = LocalPalette.current
    Box(
        modifier.clip(RoundedCornerShape(10.dp)).background(if (enabled) p.accent else p.line)
            .clickable(enabled = enabled, onClick = onClick).padding(horizontal = 18.dp, vertical = 11.dp),
        contentAlignment = Alignment.Center,
    ) { Label(text, 14.sp, if (p.light || !enabled) p.base else Color(0xFF111114), FontWeight.Medium) }
}

@Composable
fun GhostButton(text: String, color: Color = LocalPalette.current.text, onClick: () -> Unit) {
    Box(Modifier.clip(RoundedCornerShape(10.dp)).clickable(onClick = onClick).padding(horizontal = 14.dp, vertical = 11.dp)) {
        Label(text, 14.sp, color)
    }
}

@Composable
fun Field(value: String, onChange: (String) -> Unit, hint: String, password: Boolean = false, number: Boolean = false, singleLine: Boolean = true, modifier: Modifier = Modifier) {
    val p = LocalPalette.current
    BasicTextField(
        value = value,
        onValueChange = onChange,
        singleLine = singleLine,
        textStyle = TextStyle(color = p.text, fontSize = 15.sp),
        cursorBrush = SolidColor(p.accent),
        visualTransformation = if (password) PasswordVisualTransformation() else androidx.compose.ui.text.input.VisualTransformation.None,
        keyboardOptions = KeyboardOptions(keyboardType = when {
            password -> KeyboardType.Password
            number -> KeyboardType.Number
            else -> KeyboardType.Text
        }),
        modifier = modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).background(p.base).border(1.dp, p.line, RoundedCornerShape(10.dp))
            .padding(horizontal = 12.dp, vertical = 11.dp),
        decorationBox = { inner ->
            Box {
                if (value.isEmpty()) Label(hint, 15.sp, p.muted)
                inner()
            }
        },
    )
}

@Composable
fun TopBar(title: String, subtitle: String? = null, onBack: (() -> Unit)? = null, actions: @Composable () -> Unit = {}) {
    val p = LocalPalette.current
    Row(Modifier.fillMaxWidth().padding(horizontal = 6.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        if (onBack != null) IconButton(Icons.AutoMirrored.Filled.ArrowBack, "Назад", p.text, onBack) else Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f).padding(horizontal = 6.dp)) {
            Label(title, 17.sp, p.text, FontWeight.Medium, 1)
            if (subtitle != null) Label(subtitle, 12.sp, p.muted, maxLines = 1)
        }
        actions()
    }
}

@Composable
fun Dots(color: Color) {
    val t = rememberInfiniteTransition(label = "dots")
    Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
        for (i in 0 until 3) {
            val a by t.animateFloat(0.3f, 1f, infiniteRepeatable(tween(520, delayMillis = i * 140), RepeatMode.Reverse), label = "dot$i")
            Box(Modifier.size(6.dp).alpha(a).clip(CircleShape).background(color))
        }
    }
}

@Composable
fun Sheet(onDismiss: () -> Unit, content: @Composable () -> Unit) {
    val p = LocalPalette.current
    Dialog(onDismissRequest = onDismiss) {
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(p.surface).padding(20.dp)) { content() }
    }
}

// --- servers ---------------------------------------------------------------------------

@Composable
fun ServersScreen(vm: RelayViewModel) {
    val p = LocalPalette.current
    var editing by remember { mutableStateOf<ServerProfile?>(null) }
    var asking by remember { mutableStateOf<ServerProfile?>(null) }
    Column(Modifier.fillMaxSize()) {
        TopBar("Relay", "Claude Code на твоих серверах") {
            IconButton(Icons.Default.Settings, "Настройки") { vm.screen = Screen.Settings }
        }
        if (vm.notice.isNotEmpty()) Notice(vm.notice) { vm.notice = "" }
        if (vm.crash.isNotEmpty()) CrashCard(vm.crash) { vm.crash = "" }
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            if (vm.servers.isEmpty()) item {
                Column(Modifier.fillMaxWidth().padding(top = 60.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Label("Все проекты. Одно пространство.", 22.sp, p.text, FontWeight.Medium)
                    Spacer(Modifier.height(10.dp))
                    Label("Подключи сервер и продолжай с того места, где остановился.", 14.sp, p.muted)
                }
            }
            items(vm.servers, key = { it.id }) { s ->
                val online = vm.current?.id == s.id && vm.link == Link.Online
                Row(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(p.surface).border(1.dp, p.line, RoundedCornerShape(14.dp))
                        .clickable {
                            if (online) vm.screen = Screen.Sessions
                            else vm.savedPassword(s)?.let { vm.connect(s, it) } ?: run { asking = s }
                        }.padding(16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.size(42.dp).clip(RoundedCornerShape(12.dp)).background(p.mix(p.surface, p.accent, 0.18f)), contentAlignment = Alignment.Center) {
                        Label(s.name.ifEmpty { s.host }.take(2).uppercase(), 14.sp, p.accent, FontWeight.Medium)
                    }
                    Column(Modifier.weight(1f).padding(horizontal = 14.dp)) {
                        Label(s.name.ifEmpty { s.host }, 16.sp, p.text, FontWeight.Medium, 1)
                        Label("${s.user}@${s.host}" + if (s.port != 22) ":${s.port}" else "", 12.sp, p.muted, maxLines = 1)
                    }
                    if (online) Box(Modifier.size(9.dp).clip(CircleShape).background(p.green))
                    IconButton(Icons.Default.MoreVert, "Изменить") { editing = s }
                }
            }
        }
        Box(Modifier.fillMaxWidth().padding(16.dp)) {
            PrimaryButton("Добавить сервер", modifier = Modifier.fillMaxWidth()) { editing = ServerProfile() }
        }
    }
    editing?.let { s -> ServerDialog(vm, s, vm.servers.any { it.id == s.id }) { editing = null } }
    asking?.let { s ->
        var password by remember { mutableStateOf("") }
        var remember by remember { mutableStateOf(true) }
        Sheet({ asking = null }) {
            Label("Пароль для ${s.user}@${s.host}", 17.sp, p.text, FontWeight.Medium)
            Spacer(Modifier.height(12.dp))
            Field(password, { password = it }, "Пароль сервера", password = true)
            Check("Запомнить пароль", remember) { remember = it }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                GhostButton("Отмена") { asking = null }
                PrimaryButton("Подключиться", password.isNotEmpty()) {
                    if (remember) vm.saveServer(s, password, true)
                    vm.connect(s, password)
                    asking = null
                }
            }
        }
    }
}

@Composable
fun Check(text: String, value: Boolean, onChange: (Boolean) -> Unit) {
    val p = LocalPalette.current
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).clickable { onChange(!value) }.padding(vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        val knob by animateFloatAsState(if (value) 1f else 0f, tween(180), label = "toggle")
        Box(Modifier.width(36.dp).height(20.dp).clip(CircleShape).background(p.mix(p.line, p.accent, knob)), contentAlignment = Alignment.CenterStart) {
            Box(Modifier.padding(start = (3 + 16 * knob).dp).size(14.dp).clip(CircleShape).background(if (value) Color.White else p.muted))
        }
        Spacer(Modifier.width(10.dp))
        Label(text, 14.sp, p.text)
    }
}

@Composable
fun ServerDialog(vm: RelayViewModel, initial: ServerProfile, existing: Boolean, close: () -> Unit) {
    val p = LocalPalette.current
    var name by remember { mutableStateOf(initial.name) }
    var host by remember { mutableStateOf(initial.host) }
    var port by remember { mutableStateOf(initial.port.toString()) }
    var user by remember { mutableStateOf(initial.user) }
    var cwd by remember { mutableStateOf(initial.cwd) }
    var password by remember { mutableStateOf("") }
    var remember by remember { mutableStateOf(true) }
    Sheet(close) {
        Label(if (existing) "Подключение" else "Новое подключение", 19.sp, p.text, FontWeight.Medium)
        Spacer(Modifier.height(14.dp))
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Field(name, { name = it }, "Название")
            Field(host, { host = it.trim() }, "Адрес сервера")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Field(user, { user = it.trim() }, "Пользователь", modifier = Modifier.weight(2f))
                Field(port, { port = it.filter(Char::isDigit).take(5) }, "Порт", number = true, modifier = Modifier.weight(1f))
            }
            Field(password, { password = it }, if (existing) "Пароль (оставь пустым, чтобы не менять)" else "Пароль", password = true)
            Field(cwd, { cwd = it }, "Рабочая папка (~ — домашняя)")
        }
        Check("Запомнить пароль", remember) { remember = it }
        if (existing && initial.fingerprint != null) {
            Label("Ключ сервера: ${initial.fingerprint}", 11.sp, p.muted)
            GhostButton("Забыть ключ (после переустановки сервера)", p.accent) {
                vm.saveServer(initial.copy(fingerprint = null), null, false)
                close()
            }
        }
        Spacer(Modifier.height(8.dp))
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            if (existing) GhostButton("Удалить", p.red) { vm.deleteServer(initial); close() }
            Spacer(Modifier.weight(1f))
            GhostButton("Отмена", onClick = close)
            val profile = initial.copy(name = name.trim(), host = host, port = port.toIntOrNull() ?: 22, user = user.ifEmpty { "root" }, cwd = cwd.ifBlank { "~" })
            PrimaryButton(if (existing) "Сохранить" else "Подключиться", host.isNotEmpty() && (existing || password.isNotEmpty())) {
                vm.saveServer(profile, password.takeIf { it.isNotEmpty() }, remember)
                if (!existing) vm.connect(profile, password)
                close()
            }
        }
    }
}

@Composable
fun TrustDialog(request: TrustRequest, vm: RelayViewModel) {
    val p = LocalPalette.current
    Sheet({ vm.answerTrust(false) }) {
        Label("Первое подключение", 19.sp, p.text, FontWeight.Medium)
        Spacer(Modifier.height(8.dp))
        Label("Сверь отпечаток ключа ${request.server} с тем, что показывает сервер (ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub). Изменившийся ключ потом не примется молча.", 13.sp, p.muted)
        Spacer(Modifier.height(12.dp))
        Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).background(p.base).padding(12.dp)) {
            Text(request.fingerprint, color = p.text, fontSize = 13.sp, fontFamily = FontFamily.Monospace)
        }
        Spacer(Modifier.height(12.dp))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            GhostButton("Отмена") { vm.answerTrust(false) }
            PrimaryButton("Доверять") { vm.answerTrust(true) }
        }
    }
}

@Composable
fun Notice(text: String, close: () -> Unit) {
    val p = LocalPalette.current
    Row(Modifier.fillMaxWidth().padding(horizontal = 12.dp).clip(RoundedCornerShape(10.dp)).background(p.accent.copy(alpha = 0.14f)).padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
        Label(text, 13.sp, p.text, modifier = Modifier.weight(1f))
        GhostButton("OK", p.accent, close)
    }
}

/** The previous start ended in a crash: show what happened and let the user copy it. */
@Composable
fun CrashCard(text: String, close: () -> Unit) {
    val p = LocalPalette.current
    val clipboard = androidx.compose.ui.platform.LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth().padding(12.dp).clip(RoundedCornerShape(12.dp)).background(p.red.copy(alpha = 0.12f)).border(1.dp, p.red.copy(alpha = 0.4f), RoundedCornerShape(12.dp)).padding(14.dp)) {
        Label("Прошлый запуск закончился ошибкой", 14.sp, p.text, FontWeight.Medium)
        Spacer(Modifier.height(4.dp))
        Label("Скопируй отчёт и пришли его в Relay — так ошибку можно будет исправить.", 13.sp, p.muted)
        Spacer(Modifier.height(8.dp))
        Label(text.lineSequence().take(4).joinToString("\n"), 11.sp, p.muted, maxLines = 4)
        Spacer(Modifier.height(8.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            PrimaryButton(if (copied) "Скопировано" else "Скопировать отчёт") {
                clipboard.setText(androidx.compose.ui.text.AnnotatedString(text))
                copied = true
            }
            GhostButton("Закрыть", p.muted, close)
        }
    }
}

// --- sessions --------------------------------------------------------------------------

fun dayGroup(time: Double): String {
    val midnight = Calendar.getInstance().apply { set(Calendar.HOUR_OF_DAY, 0); set(Calendar.MINUTE, 0); set(Calendar.SECOND, 0) }.timeInMillis / 1000.0
    return when {
        time >= midnight -> "Сегодня"
        time >= midnight - 86400 -> "Вчера"
        time >= midnight - 6 * 86400 -> "Эта неделя"
        time >= midnight - 29 * 86400 -> "Этот месяц"
        else -> "Ранее"
    }
}

fun relative(time: Double): String {
    val s = (System.currentTimeMillis() / 1000.0 - time).toLong().coerceAtLeast(0)
    return when {
        s < 60 -> "сейчас"
        s < 3600 -> "${s / 60} мин"
        s < 86400 -> "${s / 3600} ч"
        s < 7 * 86400 -> "${s / 86400} дн"
        else -> SimpleDateFormat("d MMM", Locale("ru")).format(Date((time * 1000).toLong()))
    }
}

fun folderName(path: String) = path.trimEnd('/').substringAfterLast('/').ifEmpty { "/" }

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun SessionsScreen(vm: RelayViewModel) {
    val p = LocalPalette.current
    val server = vm.current
    var query by rememberSaveable { mutableStateOf("") }
    var searching by rememberSaveable { mutableStateOf(false) }
    var renaming by remember { mutableStateOf<Session?>(null) }
    var moving by remember { mutableStateOf<Session?>(null) }
    var naming by remember { mutableStateOf<Pair<Folder?, Session?>?>(null) }
    BackHandler { vm.screen = Screen.Servers }
    Column(Modifier.fillMaxSize().background(p.side)) {
        val status = when (vm.link) {
            Link.Online -> "на связи"
            Link.Connecting -> "подключаемся…"
            Link.Off -> "нет связи"
        }
        TopBar(server?.name?.ifEmpty { server.host } ?: "Сессии", status, onBack = { vm.screen = Screen.Servers }) {
            IconButton(Icons.Default.Search, "Поиск") { searching = !searching; if (!searching) query = "" }
            IconButton(Icons.Default.Refresh, "Обновить") { vm.refresh() }
            IconButton(Icons.Default.Settings, "Настройки") { vm.screen = Screen.Settings }
        }
        if (vm.notice.isNotEmpty()) Notice(vm.notice) { vm.notice = "" }
        AnimatedVisibility(searching) {
            Box(Modifier.padding(horizontal = 14.dp, vertical = 4.dp)) { Field(query, { query = it }, "Поиск по названию и папке") }
        }
        if (vm.link == Link.Off && server != null) {
            Box(Modifier.padding(14.dp)) {
                PrimaryButton("Подключиться снова", modifier = Modifier.fillMaxWidth()) {
                    vm.savedPassword(server)?.let { vm.connect(server, it) } ?: run { vm.screen = Screen.Servers }
                }
            }
        }
        val q = query.lowercase()
        val matches = { s: Session -> q.isEmpty() || s.title.lowercase().contains(q) || s.autoTitle.lowercase().contains(q) || s.cwd.lowercase().contains(q) }
        val known = vm.folders.map { it.id }.toSet()
        val pinned = vm.sessions.filter { it.pinned > 0 && matches(it) }.sortedByDescending { it.pinned }
        val loose = vm.sessions.filter { it.pinned == 0.0 && it.folder !in known && matches(it) }
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(horizontal = 10.dp, vertical = 6.dp)) {
            if (pinned.isNotEmpty()) {
                item(key = "h-pinned") { Heading("📌", "Закреплённые", pinned.size) }
                items(pinned, key = { "p" + it.id }) { s -> SessionRow(vm, s, { renaming = s }, { moving = s }, Modifier.animateItem()) }
            }
            for (folder in vm.folders) {
                val inside = vm.sessions.filter { it.pinned == 0.0 && it.folder == folder.id && matches(it) }
                if (q.isNotEmpty() && inside.isEmpty()) continue
                val open = folder.id !in vm.collapsed || q.isNotEmpty()
                item(key = "f-" + folder.id) {
                    FolderHeading(folder, inside.size, open, { vm.toggleFolder(folder) }, { naming = folder to null }, { vm.deleteFolder(folder) })
                }
                if (open) {
                    if (inside.isEmpty()) item(key = "e-" + folder.id) { Label("Пусто — перенеси сюда сессию через «⋮»", 12.sp, p.muted, modifier = Modifier.padding(start = 40.dp, top = 2.dp, bottom = 6.dp)) }
                    items(inside, key = { "s" + it.id }) { s -> SessionRow(vm, s, { renaming = s }, { moving = s }, Modifier.animateItem()) }
                }
            }
            var group = ""
            for (s in loose) {
                val heading = dayGroup(s.modified)
                if (heading != group) {
                    group = heading
                    item(key = "d-$heading") { Heading(null, heading, 0) }
                }
                item(key = "l" + s.id) { SessionRow(vm, s, { renaming = s }, { moving = s }, Modifier.animateItem()) }
            }
            if (vm.sessions.isEmpty()) item {
                Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) {
                    when {
                        vm.link == Link.Connecting || (vm.link == Link.Online && !vm.sessionsLoaded) -> Dots(p.muted)
                        vm.link == Link.Online -> Label("Пока нет ни одной сессии", 13.sp, p.muted)
                        else -> Label("Подключись, чтобы увидеть сессии", 13.sp, p.muted)
                    }
                }
            }
        }
        Row(Modifier.fillMaxWidth().padding(12.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Box(
                Modifier.clip(RoundedCornerShape(12.dp)).border(1.dp, p.line, RoundedCornerShape(12.dp)).clickable(enabled = vm.link == Link.Online) { naming = null to null }
                    .padding(horizontal = 16.dp, vertical = 13.dp)
            ) { Label("+ Папка", 14.sp, p.text) }
            PrimaryButton("Новая сессия", vm.link == Link.Online, Modifier.weight(1f)) { vm.newChat() }
        }
    }
    renaming?.let { s ->
        NameDialog("Название сессии", s.title, "Пусто — вернуть название от Claude", { renaming = null }) { vm.rename(s, it); renaming = null }
    }
    naming?.let { (folder, session) ->
        NameDialog(if (folder == null) "Новая папка" else "Папка", folder?.name ?: "", "Видна всем, кто подключается к серверу", { naming = null }) {
            if (it.isNotBlank()) { if (folder == null) vm.createFolder(it, session) else vm.renameFolder(folder, it) }
            naming = null
        }
    }
    moving?.let { s ->
        Sheet({ moving = null }) {
            Label("Переместить в папку", 18.sp, p.text, FontWeight.Medium)
            Spacer(Modifier.height(8.dp))
            for (f in vm.folders) {
                MenuLine("📁  " + f.name, s.folder != f.id) { vm.moveTo(s, f.id); moving = null }
            }
            if (s.folder.isNotEmpty()) MenuLine("Без папки") { vm.moveTo(s, ""); moving = null }
            HorizontalDivider(color = p.line, modifier = Modifier.padding(vertical = 6.dp))
            MenuLine("+ Новая папка…") { naming = null to s; moving = null }
        }
    }
}

@Composable
fun MenuLine(text: String, enabled: Boolean = true, color: Color = LocalPalette.current.text, onClick: () -> Unit) {
    Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).clickable(enabled = enabled, onClick = onClick).padding(horizontal = 8.dp, vertical = 12.dp)) {
        Label(text, 15.sp, if (enabled) color else LocalPalette.current.muted)
    }
}

@Composable
fun NameDialog(title: String, initial: String, hint: String, close: () -> Unit, done: (String) -> Unit) {
    val p = LocalPalette.current
    var text by remember { mutableStateOf(initial) }
    Sheet(close) {
        Label(title, 18.sp, p.text, FontWeight.Medium)
        Spacer(Modifier.height(4.dp))
        Label(hint, 12.sp, p.muted)
        Spacer(Modifier.height(12.dp))
        Field(text, { text = it }, "Название")
        Spacer(Modifier.height(12.dp))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            GhostButton("Отмена", onClick = close)
            PrimaryButton("Готово") { done(text) }
        }
    }
}

@Composable
fun Heading(icon: String?, text: String, count: Int) {
    val p = LocalPalette.current
    Row(Modifier.fillMaxWidth().padding(start = 10.dp, top = 14.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        if (icon != null) { Text(icon, fontSize = 11.sp); Spacer(Modifier.width(6.dp)) }
        Label(text.uppercase(), 11.sp, p.muted, FontWeight.Medium)
        if (count > 0) { Spacer(Modifier.width(6.dp)); Label(count.toString(), 11.sp, p.muted.copy(alpha = 0.8f)) }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun FolderHeading(folder: Folder, count: Int, open: Boolean, toggle: () -> Unit, rename: () -> Unit, delete: () -> Unit) {
    val p = LocalPalette.current
    var menu by remember { mutableStateOf(false) }
    val turn by animateFloatAsState(if (open) 90f else 0f, tween(180), label = "chevron")
    Box {
        Row(
            Modifier.fillMaxWidth().padding(top = 10.dp).clip(RoundedCornerShape(9.dp)).combinedClickable(onClick = toggle, onLongClick = { menu = true })
                .padding(horizontal = 6.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Default.KeyboardArrowRight, null, tint = p.muted, modifier = Modifier.size(18.dp).rotate(turn))
            Text(if (open) "📂" else "📁", fontSize = 14.sp)
            Spacer(Modifier.width(8.dp))
            Label(folder.name, 14.sp, p.text, FontWeight.Medium, 1, Modifier.weight(1f, fill = false))
            Spacer(Modifier.width(8.dp))
            Label(count.toString(), 12.sp, p.muted)
        }
        DropdownMenu(menu, { menu = false }, Modifier.background(p.surface)) {
            DropdownMenuItem({ Label("Переименовать папку") }, { menu = false; rename() })
            DropdownMenuItem({ Label("Удалить папку", color = p.red) }, { menu = false; delete() })
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun SessionRow(vm: RelayViewModel, s: Session, rename: () -> Unit, move: () -> Unit, modifier: Modifier = Modifier) {
    val p = LocalPalette.current
    var menu by remember { mutableStateOf(false) }
    var confirm by remember { mutableStateOf(false) }
    val busy = vm.chats[s.id]?.let { it.version >= 0 && it.transcript.busy } == true || s.id in vm.running
    val selected = vm.selected == s.id
    Box(modifier) {
        Row(
            Modifier.fillMaxWidth().padding(vertical = 1.dp).clip(RoundedCornerShape(10.dp))
                .background(if (selected) p.mix(p.side, p.line, 0.9f) else Color.Transparent)
                .combinedClickable(onClick = { vm.open(s) }, onLongClick = { menu = true })
                .padding(start = 12.dp, top = 9.dp, bottom = 9.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Label(s.title, 15.sp, if (selected) p.text else p.mix(p.text, p.muted, 0.2f), maxLines = 1)
                if (busy) Label("работает · ${folderName(s.cwd)}", 12.sp, p.accent, maxLines = 1)
                else Label("${folderName(s.cwd)} · ${relative(s.modified)}", 12.sp, p.muted, maxLines = 1)
            }
            if (busy) Dots(p.accent)
            IconButton(Icons.Default.MoreVert, "Меню") { menu = true }
        }
        DropdownMenu(menu, { menu = false }, Modifier.background(p.surface)) {
            DropdownMenuItem({ Label(if (s.pinned > 0) "Открепить" else "Закрепить сверху") }, { menu = false; vm.pin(s, s.pinned == 0.0) })
            DropdownMenuItem({ Label("Переименовать") }, { menu = false; rename() })
            DropdownMenuItem({ Label("Переместить в папку") }, { menu = false; move() })
            HorizontalDivider(color = p.line)
            DropdownMenuItem({ Label("Удалить сессию", color = p.red) }, { menu = false; confirm = true })
        }
    }
    if (confirm) {
        Sheet({ confirm = false }) {
            Label("Удалить «${s.title}»?", 17.sp, p.text, FontWeight.Medium)
            Spacer(Modifier.height(6.dp))
            Label("История этой сессии будет удалена с сервера.", 13.sp, p.muted)
            Spacer(Modifier.height(12.dp))
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                GhostButton("Отмена") { confirm = false }
                GhostButton("Удалить", p.red) { vm.delete(s); confirm = false }
            }
        }
    }
}

// --- chat ------------------------------------------------------------------------------

sealed interface Item {
    data class Text(val index: Int, val message: Message) : Item
    data class Actions(val start: Int, val messages: List<Message>) : Item
}

/** Consecutive tool calls and their results collapse into one "Действия" group. */
fun timeline(messages: List<Message>): List<Item> {
    val out = mutableListOf<Item>()
    var i = 0
    while (i < messages.size) {
        val m = messages[i]
        if (m.role == "tool" || m.role == "tool_result") {
            val start = i
            while (i < messages.size && (messages[i].role == "tool" || messages[i].role == "tool_result")) i++
            out.add(Item.Actions(start, messages.subList(start, i).toList()))
        } else {
            if (m.text.isNotBlank() || m.images > 0) out.add(Item.Text(i, m.copy()))
            i++
        }
    }
    return out
}

@Composable
fun ChatScreen(vm: RelayViewModel) {
    val p = LocalPalette.current
    val chat = vm.chat() ?: run { vm.screen = Screen.Sessions; return }
    BackHandler { vm.screen = Screen.Sessions }
    val version = chat.version
    val items = remember(version, chat.sid) { timeline(chat.transcript.messages) }
    val busy = chat.transcript.busy
    val list = rememberLazyListState()
    LaunchedEffect(items.size, version) {
        if (items.isNotEmpty()) list.animateScrollToItem(items.size + 1)
    }
    Column(Modifier.fillMaxSize()) {
        val folder = vm.sessions.firstOrNull { it.id == chat.sid }?.folder?.let { f -> vm.folders.firstOrNull { it.id == f }?.name }
        TopBar(chat.title, listOfNotNull(folder?.let { "📂 $it" }, folderName(chat.cwd)).joinToString(" · "), onBack = { vm.screen = Screen.Sessions })
        LazyColumn(Modifier.weight(1f), state = list, contentPadding = PaddingValues(horizontal = 14.dp, vertical = 8.dp)) {
            if (chat.loading) item { Row(Modifier.padding(8.dp), verticalAlignment = Alignment.CenterVertically) { Dots(p.accent); Spacer(Modifier.width(8.dp)); Label("Открываем разговор", 13.sp, p.muted) } }
            if (items.isEmpty() && !chat.loading) item {
                Column(Modifier.fillMaxWidth().padding(top = 80.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Label("С чего начнём?", 24.sp, p.text, FontWeight.Medium)
                    Spacer(Modifier.height(8.dp))
                    Label("Папка ${folderName(chat.cwd)}", 13.sp, p.muted)
                }
            }
            items(items, key = { when (it) { is Item.Text -> "m${it.index}"; is Item.Actions -> "a${it.start}" } }) { item ->
                Box(Modifier.animateItem()) {
                    when (item) {
                        is Item.Text -> if (item.message.role == "user") UserBubble(item.message) else AssistantText(item.message, busy && item.index == chat.transcript.messages.lastIndex) { path -> vm.download(path, chat.cwd) }
                        is Item.Actions -> ActionsGroup(item, busy && item.start + item.messages.size == chat.transcript.messages.size)
                    }
                }
            }
            item(key = "status") {
                Column {
                    AnimatedVisibility(busy, enter = fadeIn(), exit = fadeOut()) {
                        Row(Modifier.padding(vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                            Dots(p.accent)
                            Spacer(Modifier.width(10.dp))
                            Label(chat.transcript.status.ifEmpty { "Работает" }, 13.sp, p.muted)
                        }
                    }
                    if (chat.transcript.error.isNotEmpty()) {
                        Box(Modifier.fillMaxWidth().padding(vertical = 6.dp).clip(RoundedCornerShape(10.dp)).background(p.red.copy(alpha = 0.12f)).padding(12.dp)) {
                            Label(chat.transcript.error, 13.sp, p.text)
                        }
                    }
                    Spacer(Modifier.height(8.dp))
                }
            }
        }
        chat.permissions.firstOrNull()?.let { PermissionCard(it, vm) }
        Composer(vm, chat)
    }
}

@Composable
fun UserBubble(m: Message) {
    val p = LocalPalette.current
    Box(Modifier.fillMaxWidth().padding(vertical = 6.dp), contentAlignment = Alignment.CenterEnd) {
        Column(
            Modifier.widthIn(max = 320.dp).clip(RoundedCornerShape(16.dp)).background(p.mix(p.surface, p.accent, 0.08f))
                .border(1.dp, p.mix(p.line, p.accent, 0.14f), RoundedCornerShape(16.dp)).padding(horizontal = 14.dp, vertical = 10.dp)
        ) {
            if (m.text.isNotBlank()) Text(m.text, color = p.text, fontSize = 15.sp)
            if (m.images > 0) Label("🖼 Изображений: ${m.images}", 12.sp, p.muted)
        }
    }
}

@Composable
fun AssistantText(m: Message, streaming: Boolean = false, onFile: ((String) -> Unit)? = null) {
    val p = LocalPalette.current
    Box(Modifier.fillMaxWidth().padding(vertical = 6.dp)) { MarkdownText(m.text, p, Modifier.fillMaxWidth(), streaming, onFile) }
}

@Composable
fun ActionsGroup(item: Item.Actions, running: Boolean) {
    val p = LocalPalette.current
    // The live group stays open so you can follow along; finished ones fold into one line.
    var open by rememberSaveable(item.start) { mutableStateOf(running) }
    LaunchedEffect(running) { if (running) open = true }
    val calls = item.messages.filter { it.role == "tool" }
    val failed = item.messages.any { it.role == "tool_result" && it.failed }
    val turn by animateFloatAsState(if (open) 90f else 0f, tween(200), label = "turn")
    Column(
        Modifier.fillMaxWidth().padding(vertical = 6.dp).clip(RoundedCornerShape(12.dp)).background(p.surface)
            .border(1.dp, if (failed) p.red.copy(alpha = 0.5f) else p.line, RoundedCornerShape(12.dp)).animateContentSize(tween(240))
    ) {
        Row(Modifier.fillMaxWidth().clickable { open = !open }.padding(horizontal = 12.dp, vertical = 11.dp), verticalAlignment = Alignment.CenterVertically) {
            if (running) { Dots(p.accent); Spacer(Modifier.width(8.dp)) }
            val count = calls.size
            val word = when { count % 10 == 1 && count % 100 != 11 -> "действие"; count % 10 in 2..4 && count % 100 !in 12..14 -> "действия"; else -> "действий" }
            Label("Действия · $count $word" + if (failed) " · есть ошибки" else "", 13.sp, if (failed) p.red else p.muted, modifier = Modifier.weight(1f))
            Icon(Icons.Default.KeyboardArrowRight, null, tint = p.muted, modifier = Modifier.size(20.dp).rotate(turn))
        }
        AnimatedVisibility(open, enter = expandVertically(tween(240)) + fadeIn(tween(240)), exit = shrinkVertically(tween(200)) + fadeOut(tween(160))) {
            Column(Modifier.padding(start = 12.dp, end = 12.dp, bottom = 10.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                for (call in calls) {
                    val result = item.messages.firstOrNull { it.role == "tool_result" && it.toolId == call.toolId && call.toolId.isNotEmpty() }
                    ToolCall(call, result, running)
                }
            }
        }
    }
}

@Composable
fun ToolCall(call: Message, result: Message?, running: Boolean = false) {
    val p = LocalPalette.current
    var full by remember { mutableStateOf(false) }
    // New actions slide in instead of popping up.
    val appear = remember { MutableTransitionState(false).apply { targetState = true } }
    AnimatedVisibility(appear, enter = fadeIn(tween(260)) + slideInVertically(tween(260)) { it / 3 }) {
        Column(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).background(p.base)
                .border(1.dp, when { result?.failed == true -> p.red.copy(alpha = 0.45f); result == null && running -> p.accent.copy(alpha = 0.45f); else -> p.line }, RoundedCornerShape(10.dp))
                .clickable { full = !full }.padding(10.dp).animateContentSize(tween(220))
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                StatusMark(when { result == null && running -> 0; result?.failed == true -> 2; else -> 1 })
                Spacer(Modifier.width(9.dp))
                val heading = call.input.optString("description").trim().takeIf { it.isNotEmpty() && !call.tool.startsWith("mcp__relay_cluster__") } ?: toolName(call.tool)
                Label(heading, 13.5.sp, if (result?.failed == true) p.red else p.text, FontWeight.Medium, maxLines = 2)
            }
            val line = commandLine(call)
            if (line.isNotEmpty()) {
                Text(line, color = p.mix(p.text, p.muted, 0.45f), fontSize = 12.sp, fontFamily = FontFamily.Monospace, maxLines = if (full) 20 else 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(start = 27.dp, top = 3.dp))
            }
            Column(Modifier.padding(start = 27.dp, top = 6.dp)) {
                if (call.tool == "Edit" || call.tool == "MultiEdit") {
                    val pairs = if (call.tool == "MultiEdit") call.input.optJSONArray("edits")?.let { a -> (0 until a.length()).mapNotNull { a.optJSONObject(it) }.map { it.optString("old_string") to it.optString("new_string") } }.orEmpty()
                        else listOf(call.input.optString("old_string") to call.input.optString("new_string"))
                    val lines = pairs.flatMap { (old, new) -> old.lines().filter { it.isNotBlank() }.take(if (full) 40 else 3).map { "−" to it } + new.lines().filter { it.isNotBlank() }.take(if (full) 40 else 3).map { "+" to it } }
                    for ((sign, text) in lines.take(if (full) 200 else 8)) {
                        val color = if (sign == "+") p.green else p.red
                        Text("$sign ${text.trimEnd()}", color = color, fontSize = 11.5.sp, fontFamily = FontFamily.Monospace, maxLines = 1, overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.fillMaxWidth().padding(vertical = 1.dp).clip(RoundedCornerShape(4.dp)).background(color.copy(alpha = 0.1f)).padding(horizontal = 6.dp, vertical = 1.dp))
                    }
                } else if (result == null && running) {
                    val dots by rememberInfiniteTransition(label = "run").animateFloat(0f, 3f, infiniteRepeatable(tween(1200, easing = LinearEasing)), label = "dots")
                    Label("выполняется" + ".".repeat(1 + dots.toInt().coerceAtMost(2)), 12.sp, p.accent)
                } else if (result != null && result.text.isNotBlank() && call.tool != "Read") {
                    val all = result.text.trimEnd().lines().filter { it.isNotBlank() }
                    val shown = if (full) all.takeLast(400) else all.takeLast(4)
                    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(7.dp)).background(if (result.failed) p.red.copy(alpha = 0.08f) else p.side).padding(horizontal = 9.dp, vertical = 6.dp)) {
                        if (all.size > shown.size) Label("… ещё ${lines(all.size - shown.size)} выше · нажми, чтобы раскрыть", 10.5.sp, p.muted)
                        Text(shown.joinToString("\n"), color = if (result.failed) p.red else p.mix(p.text, p.muted, 0.3f), fontSize = 11.5.sp, fontFamily = FontFamily.Monospace)
                    }
                }
            }
        }
    }
}

/** 0 — a spinning arc while it runs, 1 — green tick, 2 — red cross. */
@Composable
fun StatusMark(state: Int) {
    val p = LocalPalette.current
    val spin by rememberInfiniteTransition(label = "spin").animateFloat(0f, 360f, infiniteRepeatable(tween(900, easing = LinearEasing)), label = "angle")
    val color = when (state) { 0 -> p.accent; 1 -> p.green; else -> p.red }
    androidx.compose.foundation.Canvas(Modifier.size(18.dp)) {
        val r = size.minDimension / 2f - 1.5f
        val stroke = androidx.compose.ui.graphics.drawscope.Stroke(width = 1.7.dp.toPx(), cap = androidx.compose.ui.graphics.StrokeCap.Round)
        when (state) {
            0 -> {
                drawCircle(color.copy(alpha = 0.2f), r, style = stroke)
                drawArc(color, spin, 250f, false, topLeft = center - androidx.compose.ui.geometry.Offset(r, r), size = androidx.compose.ui.geometry.Size(r * 2, r * 2), style = stroke)
            }
            1 -> {
                drawCircle(color.copy(alpha = 0.18f), r)
                val path = androidx.compose.ui.graphics.Path().apply { moveTo(center.x - r * 0.42f, center.y); lineTo(center.x - r * 0.1f, center.y + r * 0.33f); lineTo(center.x + r * 0.45f, center.y - r * 0.3f) }
                drawPath(path, color, style = stroke)
            }
            else -> {
                drawCircle(color.copy(alpha = 0.18f), r)
                val d = r * 0.36f
                drawLine(color, center - androidx.compose.ui.geometry.Offset(d, d), center + androidx.compose.ui.geometry.Offset(d, d), stroke.width, androidx.compose.ui.graphics.StrokeCap.Round)
                drawLine(color, center + androidx.compose.ui.geometry.Offset(d, -d), center + androidx.compose.ui.geometry.Offset(-d, d), stroke.width, androidx.compose.ui.graphics.StrokeCap.Round)
            }
        }
    }
}

fun lines(n: Int) = "$n " + when { n % 10 == 1 && n % 100 != 11 -> "строка"; n % 10 in 2..4 && n % 100 !in 12..14 -> "строки"; else -> "строк" }

/** What exactly ran: `$ ./gradlew build`, a file, a search. */
fun commandLine(call: Message): String {
    val i = call.input
    return when (call.tool) {
        "Bash", "mcp__relay_desktop__run_local_command" -> "$ " + i.optString("command").trim().lines().joinToString(" ⏎ ")
        "Grep" -> "«${i.optString("pattern")}»" + i.optString("path").let { if (it.isEmpty()) "" else " в $it" }
        "Read", "Write", "Edit", "MultiEdit" -> i.optString("file_path")
        "Glob" -> i.optString("pattern")
        "SlashCommand" -> i.optString("command")
        "WebSearch" -> i.optString("query")
        "WebFetch" -> i.optString("url")
        else -> if (call.tool.startsWith("mcp__relay_cluster__")) "" else toolSummary(call.tool, i)
    }
}

@Composable
fun PermissionCard(permission: Permission, vm: RelayViewModel) {
    val p = LocalPalette.current
    Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp).clip(RoundedCornerShape(14.dp)).background(p.surface).border(1.dp, p.accent.copy(alpha = 0.6f), RoundedCornerShape(14.dp)).padding(14.dp)) {
        Label("Claude просит разрешение", 13.sp, p.muted)
        Spacer(Modifier.height(4.dp))
        Label(toolName(permission.tool), 15.sp, p.text, FontWeight.Medium)
        val summary = toolSummary(permission.tool, permission.input)
        if (summary.isNotEmpty()) {
            Spacer(Modifier.height(6.dp))
            Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).background(p.base).padding(10.dp)) {
                Text(summary, color = p.text, fontSize = 12.sp, fontFamily = FontFamily.Monospace, maxLines = 6, overflow = TextOverflow.Ellipsis)
            }
        }
        Spacer(Modifier.height(10.dp))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            GhostButton("Отклонить", p.red) { vm.permit(permission, false) }
            PrimaryButton("Разрешить") { vm.permit(permission, true) }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun TransferCard(vm: RelayViewModel, t: Transfer) {
    val p = LocalPalette.current
    val progress by animateFloatAsState(if (t.size > 0) (t.received.toFloat() / t.size).coerceIn(0f, 1f) else 0f, tween(200), label = "progress")
    Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 6.dp).clip(RoundedCornerShape(12.dp)).background(p.side)
        .border(1.dp, if (t.error.isNotEmpty()) p.red.copy(alpha = 0.5f) else p.line, RoundedCornerShape(12.dp)).padding(horizontal = 12.dp, vertical = 9.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            StatusMark(when { t.error.isNotEmpty() -> 2; t.done -> 1; else -> 0 })
            Spacer(Modifier.width(9.dp))
            Column(Modifier.weight(1f)) {
                Label(t.name, 13.5.sp, p.text, FontWeight.Medium, maxLines = 1)
                Label(when {
                    t.error.isNotEmpty() -> t.error
                    t.done -> "В «Загрузки/Relay» · ${megabytes(t.size)}"
                    t.size > 0 -> "${megabytes(t.received)} из ${megabytes(t.size)}"
                    else -> "Ищем файл на сервере…"
                }, 12.sp, if (t.error.isNotEmpty()) p.red else p.muted, maxLines = 2)
            }
            if (t.done) {
                GhostButton("Открыть", p.accent) { vm.openTransfer(t) }
                IconButton(Icons.Default.Share, "Поделиться") { vm.shareTransfer(t) }
            }
            IconButton(Icons.Default.Close, if (t.done || t.error.isNotEmpty()) "Убрать" else "Отменить") { vm.dismiss(t) }
        }
        if (!t.done && t.error.isEmpty() && t.size > 0) {
            Box(Modifier.padding(top = 6.dp).fillMaxWidth().height(3.dp).clip(RoundedCornerShape(2.dp)).background(p.line)) {
                Box(Modifier.fillMaxWidth(progress).fillMaxHeight().background(p.accent))
            }
        }
    }
}

fun megabytes(bytes: Long) = if (bytes < 1024 * 1024) "${(bytes + 1023) / 1024} КБ" else "%.1f МБ".format(bytes / 1048576.0)

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun Composer(vm: RelayViewModel, chat: Chat) {
    val p = LocalPalette.current
    val busy = chat.version >= 0 && chat.transcript.busy
    for (t in vm.transfers.take(3)) key(t.id) { TransferCard(vm, t) }
    val online = vm.link == Link.Online
    val suggestions = slashMatches(chat.draft, vm.commands)
    if (chat.queued.isNotEmpty()) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 6.dp).clip(RoundedCornerShape(12.dp)).background(p.side).border(1.dp, p.line, RoundedCornerShape(12.dp)).padding(10.dp)) {
            Label("В очереди: ${chat.queued.size} — Claude ответит по порядку, даже если закрыть приложение", 12.sp, p.muted)
            for ((i, q) in chat.queued.withIndex()) Label("${i + 1}  $q", 13.sp, p.text, maxLines = 1, modifier = Modifier.padding(top = 4.dp))
        }
    }
    AnimatedVisibility(suggestions.isNotEmpty()) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 6.dp).heightIn(max = 260.dp).clip(RoundedCornerShape(12.dp)).background(p.side).border(1.dp, p.line, RoundedCornerShape(12.dp)).verticalScroll(rememberScrollState()).padding(4.dp)) {
            for (c in suggestions.take(12)) {
                Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).clickable { chat.draft = "/${c.name} " }.padding(horizontal = 10.dp, vertical = 7.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Label("/${c.name}", 14.sp, p.accent, FontWeight.Medium)
                        if (c.hint.isNotEmpty()) Label("  ${c.hint}", 12.sp, p.muted, maxLines = 1)
                    }
                    if (c.description.isNotEmpty()) Label(c.description, 12.sp, p.muted, maxLines = 2)
                }
            }
            Label("/model, /mode и /stop срабатывают сразу, даже во время ответа", 11.sp, p.muted, modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp))
        }
    }
    Column(Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 8.dp).clip(RoundedCornerShape(16.dp)).background(p.surface).border(1.dp, p.line, RoundedCornerShape(16.dp)).padding(start = 14.dp, end = 8.dp, top = 10.dp, bottom = 6.dp)) {
        BasicTextField(
            value = chat.draft,
            onValueChange = { chat.draft = it },
            textStyle = TextStyle(color = p.text, fontSize = 15.sp),
            cursorBrush = SolidColor(p.accent),
            modifier = Modifier.fillMaxWidth().heightIn(min = 24.dp, max = 160.dp),
            decorationBox = { inner -> Box { if (chat.draft.isEmpty()) Label(when { !online -> "Нет связи с сервером"; busy && vm.canSend(chat) -> "Добавь сообщение — Claude ответит следом…"; busy -> "Claude отвечает…"; else -> "Напиши задачу… или / для команд" }, 15.sp, p.muted); inner() } },
        )
        Row(Modifier.fillMaxWidth().padding(top = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            FlowRow(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(2.dp)) {
                val models = vm.models.ifEmpty { listOf(ModelInfo("default", "Модель сервера", ""), ModelInfo("fable", "Fable", ""), ModelInfo("opus", "Opus", ""), ModelInfo("sonnet", "Sonnet", ""), ModelInfo("haiku", "Haiku", "")) }
                Selector(modelLabel(vm.models, chat.model), models.map { it.value to if (it.value == "default") "Авто · ${it.description}".trimEnd(' ', '·') else it.name }) { chat.model = it; vm.liveChange(chat) }
                Selector(when (chat.mode) { "auto" -> "Авто"; "acceptEdits" -> "Правки файлов"; "plan" -> "План"; else -> "С подтверждением" },
                    listOf("auto" to "Авто", "manual" to "С подтверждением", "acceptEdits" to "Правки файлов", "plan" to "Планирование"), accent = chat.mode == "auto") { chat.mode = it; vm.liveChange(chat) }
                Selector("Глубина: " + if (chat.effort == "default") "авто" else chat.effort,
                    listOf("default" to "Автоматически", "low" to "Низкая", "medium" to "Средняя", "high" to "Высокая", "xhigh" to "Очень высокая", "max" to "Максимальная")) { chat.effort = it }
            }
            val ready = chat.draft.isNotBlank() && (vm.canSend(chat) || chat.draft.trim().startsWith("/"))
            if (busy) {
                Box(
                    Modifier.size(38.dp).clip(CircleShape).background(p.line).clickable { vm.stop() },
                    contentAlignment = Alignment.Center,
                ) { Box(Modifier.size(12.dp).clip(RoundedCornerShape(3.dp)).background(p.text)) }
                Spacer(Modifier.width(6.dp))
            }
            if (!busy || ready) {
                Box(
                    Modifier.size(38.dp).clip(CircleShape).background(if (ready) p.accent else p.line)
                        .clickable(enabled = ready) { vm.submit() },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(Icons.AutoMirrored.Filled.Send, "Отправить", tint = if (ready) (if (p.light) p.base else Color(0xFF111114)) else p.muted, modifier = Modifier.size(18.dp))
                }
            }
        }
    }
}

@Composable
fun Selector(label: String, options: List<Pair<String, String>>, accent: Boolean = false, choose: (String) -> Unit) {
    val p = LocalPalette.current
    var open by remember { mutableStateOf(false) }
    Box {
        Row(Modifier.clip(RoundedCornerShape(8.dp)).clickable { open = true }.padding(horizontal = 8.dp, vertical = 7.dp), verticalAlignment = Alignment.CenterVertically) {
            Label(label, 12.5.sp, if (accent) p.accent else p.muted, maxLines = 1)
            Icon(Icons.Default.KeyboardArrowDown, null, tint = p.muted, modifier = Modifier.size(16.dp))
        }
        DropdownMenu(open, { open = false }, Modifier.background(p.surface)) {
            for ((id, name) in options) DropdownMenuItem({ Label(name) }, { open = false; choose(id) })
        }
    }
}

// --- settings --------------------------------------------------------------------------

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SettingsScreen(vm: RelayViewModel) {
    val p = LocalPalette.current
    val back = { vm.screen = if (vm.current != null && vm.link != Link.Off) Screen.Sessions else Screen.Servers }
    BackHandler { back() }
    var name by remember { mutableStateOf(vm.actor) }
    Column(Modifier.fillMaxSize()) {
        TopBar("Настройки", "Оформление и имя в журнале команды", onBack = back)
        LazyColumn(contentPadding = PaddingValues(16.dp)) {
            item { Label("Цветовая тема", 15.sp, p.text, FontWeight.Medium); Spacer(Modifier.height(10.dp)) }
            item {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    for (t in THEMES) ThemeTile(t, vm.theme == t.id) { vm.chooseTheme(t.id) }
                }
            }
            item {
                Spacer(Modifier.height(24.dp))
                Label("Твоё имя", 15.sp, p.text, FontWeight.Medium)
                Spacer(Modifier.height(4.dp))
                Label("Видно на доске задач и в общих названиях сессий", 12.sp, p.muted)
                Spacer(Modifier.height(8.dp))
                Field(name, { name = it; vm.chooseActor(it) }, "Например, champ")
                if (vm.link == Link.Online) {
                    Spacer(Modifier.height(24.dp))
                    Label("Claude Code на сервере", 15.sp, p.text, FontWeight.Medium)
                    Spacer(Modifier.height(4.dp))
                    Label("${vm.claudeVersion.ifEmpty { "версия неизвестна" }} · обновляется сам раз в 6 часов, когда Claude ничего не делает", 12.sp, p.muted)
                    Spacer(Modifier.height(8.dp))
                    PrimaryButton(if (vm.updating) "Обновляем…" else "Обновить сейчас", enabled = !vm.updating) { vm.updateClaude() }
                    if (vm.notice.isNotEmpty()) { Spacer(Modifier.height(8.dp)); Label(vm.notice, 12.sp, p.muted) }
                }
                Spacer(Modifier.height(24.dp))
                Label("Relay для Android 0.2.2 · тот же сервер и тот же помощник, что у десктопа", 12.sp, p.muted)
            }
        }
    }
}

@Composable
fun ThemeTile(t: ThemeInfo, selected: Boolean, choose: () -> Unit) {
    val p = LocalPalette.current
    val c = palette(t.id)
    Column(
        Modifier.width(100.dp).clip(RoundedCornerShape(12.dp)).background(c.base)
            .border(if (selected) 2.dp else 1.dp, if (selected) p.accent else p.line, RoundedCornerShape(12.dp)).clickable(onClick = choose).padding(8.dp)
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(5.dp)) {
            Box(Modifier.width(14.dp).height(30.dp).clip(RoundedCornerShape(4.dp)).background(c.rail))
            Column(verticalArrangement = Arrangement.spacedBy(5.dp)) {
                Box(Modifier.width(56.dp).height(12.dp).clip(RoundedCornerShape(3.dp)).background(c.surface))
                Box(Modifier.width(30.dp).height(5.dp).clip(RoundedCornerShape(3.dp)).background(c.accent))
            }
        }
        Spacer(Modifier.height(8.dp))
        Text(t.name, color = c.text, fontSize = 12.sp, modifier = Modifier.align(Alignment.CenterHorizontally))
    }
}
