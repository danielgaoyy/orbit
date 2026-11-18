package dev.orbit.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
import uniffi.orbit_ffi_uniffi.FfiConnectionState
import uniffi.orbit_ffi_uniffi.FfiDiffLineKind
import uniffi.orbit_ffi_uniffi.FfiHighlightedLine
import uniffi.orbit_ffi_uniffi.FfiInlineSpan
import uniffi.orbit_ffi_uniffi.FfiProviderKind
import uniffi.orbit_ffi_uniffi.FfiRichBlock
import uniffi.orbit_ffi_uniffi.FfiTerminalRow
import uniffi.orbit_ffi_uniffi.FfiTimelineItem

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            MaterialTheme(
                colorScheme = darkColorScheme(
                    background = Color(0xFF090D16),
                    surface = Color(0xFF111827),
                    primary = Color(0xFF60A5FA),
                )
            ) {
                Surface(modifier = Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    OrbitMobileRoot()
                }
            }
        }
    }
}

private fun parseHexColor(hex: String, fallback: Color = Color(0xFFE2E8F0)): Color {
    val clean = hex.removePrefix("#")
    return runCatching {
        Color(clean.toLong(16) or 0xFF000000L)
    }.getOrDefault(fallback)
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun OrbitMobileRoot(store: OrbitAndroidStore = viewModel()) {
    val ui by store.state.collectAsState()
    var activeTab by remember { mutableIntStateOf(0) }

    val activeCwd = ui.hostDetail?.workspaces?.firstOrNull()?.cwd
        ?: ui.hostDetail?.agents?.firstOrNull()?.cwd
        ?: "~"

    Scaffold(
        modifier = Modifier.imePadding(),
        topBar = {
            TopAppBar(
                title = {
                    val hostTitle = ui.hostDetail?.host?.displayName ?: "Orbit Fleet"
                    Text(hostTitle, fontWeight = FontWeight.SemiBold)
                }
            )
        },
        bottomBar = {
            NavigationBar {
                listOf("Fleet", "Stream", "Diff", "PTY").forEachIndexed { index, label ->
                    NavigationBarItem(
                        selected = activeTab == index,
                        onClick = { activeTab = index },
                        icon = { Text(label.take(1), fontWeight = FontWeight.Bold) },
                        label = { Text(label) },
                    )
                }
            }
        }
    ) { innerPadding ->
        Column(modifier = Modifier.fillMaxSize().padding(innerPadding)) {
            when (activeTab) {
                0 -> FleetScreen(ui = ui, store = store, onOpenStream = { activeTab = 1 })
                1 -> StreamScreen(ui = ui, store = store)
                2 -> DiffScreen(ui = ui, store = store, cwd = activeCwd)
                else -> TerminalScreen(ui = ui, store = store, cwd = activeCwd)
            }
        }
    }
}

@Composable
private fun FleetScreen(ui: OrbitUiState, store: OrbitAndroidStore, onOpenStream: () -> Unit) {
    var pairUri by remember { mutableStateOf("") }
    var newCwd by remember { mutableStateOf("~") }
    var newPrompt by remember { mutableStateOf("") }
    var selectedProvider by remember { mutableStateOf(FfiProviderKind.ANTIGRAVITY) }

    LazyColumn(
        modifier = Modifier.fillMaxSize().padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        item {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(
                    value = pairUri,
                    onValueChange = { pairUri = it },
                    placeholder = { Text("orbit://pair/...") },
                    singleLine = true,
                    modifier = Modifier.weight(1f)
                )
                Button(onClick = {
                    store.pairHost(pairUri)
                    pairUri = ""
                }) {
                    Text("Pair")
                }
            }
            ui.lastError?.let { err ->
                Text(err, color = Color(0xFFF87171), fontSize = 12.sp, modifier = Modifier.padding(top = 4.dp))
            }
        }

        items(ui.hosts, key = { it.hostId }) { host ->
            val selected = host.hostId == ui.selectedHostId
            Card(
                modifier = Modifier.fillMaxWidth().clickable { store.selectHost(host.hostId) },
                colors = CardDefaults.cardColors(
                    containerColor = if (selected) Color(0xFF1E293B) else Color(0xFF111827)
                )
            ) {
                Column(modifier = Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text(host.displayName, fontWeight = FontWeight.Bold)
                        val badgeColor = when (host.connectionState) {
                            FfiConnectionState.ONLINE_DIRECT -> Color(0xFF4ADE80)
                            FfiConnectionState.ONLINE_RELAY -> Color(0xFFC084FC)
                            else -> Color(0xFF94A3B8)
                        }
                        Text(host.connectionState.name, color = badgeColor, fontSize = 11.sp)
                    }
                    Text(
                        "${host.agentCount} agents · ${host.pendingApprovalCount} awaiting approval",
                        color = Color(0xFF94A3B8),
                        fontSize = 12.sp
                    )
                }
            }
        }

        ui.hostDetail?.let { detail ->
            item {
                HorizontalDivider()
                Text("Agent Sessions on ${detail.host.displayName}", fontWeight = FontWeight.SemiBold)
            }

            items(detail.agents, key = { it.agentId }) { ag ->
                Card(
                    modifier = Modifier.fillMaxWidth().clickable {
                        store.selectAgent(ag.agentId)
                        onOpenStream()
                    }
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(12.dp),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text(ag.title, fontWeight = FontWeight.Medium)
                            Text("${ag.providerLabel} · ${ag.cwd}", fontSize = 11.sp, color = Color(0xFF94A3B8))
                        }
                        Text(
                            ag.statusLabel,
                            fontSize = 11.sp,
                            color = if (ag.isAwaitingPermission) Color(0xFFFBBF24) else Color(0xFF60A5FA)
                        )
                    }
                }
            }

            item {
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text("Launch New Agent", fontWeight = FontWeight.SemiBold)
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            FfiProviderKind.entries.forEach { p ->
                                FilterChip(
                                    selected = selectedProvider == p,
                                    onClick = { selectedProvider = p },
                                    label = { Text(p.name.lowercase().replace("_", " ")) }
                                )
                            }
                        }
                        OutlinedTextField(
                            value = newCwd,
                            onValueChange = { newCwd = it },
                            label = { Text("Workspace CWD") },
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth()
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                            OutlinedTextField(
                                value = newPrompt,
                                onValueChange = { newPrompt = it },
                                placeholder = { Text("Task prompt...") },
                                modifier = Modifier.weight(1f)
                            )
                            Button(onClick = {
                                store.createAgent(newCwd, selectedProvider, newPrompt)
                                newPrompt = ""
                                onOpenStream()
                            }) {
                                Text("Start")
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun StreamScreen(ui: OrbitUiState, store: OrbitAndroidStore) {
    var composer by remember { mutableStateOf("") }

    Column(modifier = Modifier.fillMaxSize()) {
        LazyColumn(
            modifier = Modifier.weight(1f).fillMaxWidth().padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp)
        ) {
            items(ui.timelineItems) { item ->
                when (item) {
                    is FfiTimelineItem.UserPrompt -> {
                        Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF172554))) {
                            Column(modifier = Modifier.padding(12.dp)) {
                                RichBlocksList(item.blocks)
                            }
                        }
                    }
                    is FfiTimelineItem.AssistantThinking -> {
                        Text(
                            "Thinking: ${item.text.take(240)}",
                            fontSize = 11.sp,
                            fontFamily = FontFamily.Monospace,
                            color = Color(0xFF94A3B8),
                            modifier = Modifier.padding(horizontal = 4.dp)
                        )
                    }
                    is FfiTimelineItem.AssistantMessage -> {
                        Card {
                            Column(modifier = Modifier.padding(12.dp)) {
                                RichBlocksList(item.blocks)
                            }
                        }
                    }
                    is FfiTimelineItem.ToolCall -> {
                        Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF0B0F19))) {
                            Column(modifier = Modifier.padding(10.dp)) {
                                Text(item.summaryTitle, fontFamily = FontFamily.Monospace, fontSize = 12.sp)
                                item.previewBody?.let {
                                    Text(it, fontFamily = FontFamily.Monospace, fontSize = 11.sp, color = Color(0xFF94A3B8))
                                }
                            }
                        }
                    }
                    is FfiTimelineItem.PermissionRequest -> {
                        Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF422006))) {
                            Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                Text(item.title, fontWeight = FontWeight.Bold, color = Color(0xFFFBBF24))
                                Text(item.commandOrPath, fontFamily = FontFamily.Monospace, fontSize = 12.sp)
                                if (item.resolvedDecision != null) {
                                    Text("Resolved: ${item.resolvedDecision}", color = Color(0xFF4ADE80), fontSize = 12.sp)
                                } else {
                                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                        val opts = item.allowedDecisions.ifEmpty { listOf("allow", "deny") }
                                        opts.forEach { dec ->
                                            Button(
                                                onClick = { store.resolvePermission(item.requestId, dec) },
                                                colors = ButtonDefaults.buttonColors(
                                                    containerColor = if (dec.contains("deny", true)) Color(0xFFDC2626) else Color(0xFF2563EB)
                                                )
                                            ) {
                                                Text(dec)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    is FfiTimelineItem.QuestionRequest -> {
                        Card(colors = CardDefaults.cardColors(containerColor = Color(0xFF082F49))) {
                            Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                item.questions.forEach { q ->
                                    Text(q.question, fontWeight = FontWeight.Medium)
                                    Row(
                                        modifier = Modifier.horizontalScroll(rememberScrollState()),
                                        horizontalArrangement = Arrangement.spacedBy(6.dp)
                                    ) {
                                        q.options.forEach { opt ->
                                            OutlinedButton(
                                                enabled = item.resolvedAnswers == null,
                                                onClick = { store.answerQuestion(item.requestId, opt) }
                                            ) {
                                                Text(opt, fontSize = 12.sp)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    is FfiTimelineItem.Notice -> {
                        Text(item.message, fontSize = 12.sp, color = Color(0xFF94A3B8))
                    }
                }
            }
        }

        Row(
            modifier = Modifier.fillMaxWidth().padding(10.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            OutlinedTextField(
                value = composer,
                onValueChange = { composer = it },
                placeholder = { Text("Send message or steer...") },
                modifier = Modifier.weight(1f)
            )
            OutlinedButton(onClick = { store.interruptAgent() }) {
                Text("Stop")
            }
            Button(onClick = {
                store.sendPrompt(composer)
                composer = ""
            }) {
                Text("Send")
            }
        }
    }
}

@Composable
private fun RichBlocksList(blocks: List<FfiRichBlock>) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        blocks.forEach { blk ->
            when (blk) {
                is FfiRichBlock.Heading -> {
                    Text(buildInlineSpans(blk.spans), fontWeight = FontWeight.Bold, fontSize = 16.sp)
                }
                is FfiRichBlock.Paragraph -> {
                    Text(buildInlineSpans(blk.spans), fontSize = 14.sp)
                }
                is FfiRichBlock.BlockQuote -> {
                    Text(buildInlineSpans(blk.spans), fontSize = 13.sp, color = Color(0xFF94A3B8))
                }
                is FfiRichBlock.ListItem -> {
                    Text("• ${buildInlineSpans(blk.spans)}", fontSize = 14.sp)
                }
                is FfiRichBlock.CodeBlock -> {
                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .background(Color(0xFF070A12), RoundedCornerShape(8.dp))
                            .horizontalScroll(rememberScrollState())
                            .padding(10.dp)
                    ) {
                        blk.lines.forEach { ln ->
                            Text(buildHighlightedCodeLine(ln), fontFamily = FontFamily.Monospace, fontSize = 12.sp)
                        }
                    }
                }
                FfiRichBlock.HorizontalRule -> HorizontalDivider()
            }
        }
    }
}

private fun buildInlineSpans(spans: List<FfiInlineSpan>): AnnotatedString = buildAnnotatedString {
    spans.forEach { sp ->
        withStyle(
            SpanStyle(
                fontWeight = if (sp.bold) FontWeight.Bold else FontWeight.Normal,
                fontStyle = if (sp.italic) FontStyle.Italic else FontStyle.Normal,
                fontFamily = if (sp.inlineCode) FontFamily.Monospace else FontFamily.Default,
                color = if (sp.inlineCode) Color(0xFF38BDF8) else Color(0xFFF1F5F9),
            )
        ) {
            append(sp.text)
        }
    }
}

private fun buildHighlightedCodeLine(line: FfiHighlightedLine): AnnotatedString = buildAnnotatedString {
    line.tokens.forEach { tok ->
        withStyle(
            SpanStyle(
                color = parseHexColor(tok.fgHex),
                fontWeight = if (tok.bold) FontWeight.Bold else FontWeight.Normal,
            )
        ) {
            append(tok.text)
        }
    }
}

@Composable
private fun DiffScreen(ui: OrbitUiState, store: OrbitAndroidStore, cwd: String) {
    LaunchedEffect(ui.selectedHostId, cwd) {
        store.loadGitReview(cwd)
    }
    LazyColumn(
        modifier = Modifier.fillMaxSize().padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        val files = ui.gitReview?.diffFiles.orEmpty()
        if (files.isEmpty()) {
            item {
                Text("Working tree clean.", color = Color(0xFF94A3B8))
            }
        } else {
            items(files, key = { it.path }) { file ->
                Card {
                    Column(modifier = Modifier.padding(10.dp)) {
                        Text(
                            "${file.path}  +${file.additions} -${file.deletions}",
                            fontFamily = FontFamily.Monospace,
                            fontWeight = FontWeight.SemiBold,
                            fontSize = 12.sp
                        )
                        Spacer(modifier = Modifier.height(6.dp))
                        file.lines.forEach { ln ->
                            val bg = when (ln.kind) {
                                FfiDiffLineKind.ADDED -> Color(0xFF052E16)
                                FfiDiffLineKind.REMOVED -> Color(0xFF450A0A)
                                FfiDiffLineKind.HUNK_HEADER -> Color(0xFF0C4A6E)
                                else -> Color.Transparent
                            }
                            Text(
                                text = ln.content,
                                fontFamily = FontFamily.Monospace,
                                fontSize = 11.5.sp,
                                modifier = Modifier.fillMaxWidth().background(bg).padding(horizontal = 4.dp, vertical = 1.dp)
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun TerminalScreen(ui: OrbitUiState, store: OrbitAndroidStore, cwd: String) {
    var cmd by remember { mutableStateOf("") }

    LaunchedEffect(ui.selectedHostId, cwd) {
        store.startTerminal(cwd)
    }

    Column(modifier = Modifier.fillMaxSize().padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        LazyColumn(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .background(Color(0xFF05080F), RoundedCornerShape(10.dp))
                .horizontalScroll(rememberScrollState())
                .padding(8.dp)
        ) {
            val rows: List<FfiTerminalRow> = ui.terminalScreen?.rowsData.orEmpty()
            items(rows, key = { it.rowIndex.toInt() }) { row ->
                val styled = buildAnnotatedString {
                    row.spans.forEach { sp ->
                        withStyle(SpanStyle(color = parseHexColor(sp.fgHex))) {
                            append(sp.text)
                        }
                    }
                }
                Text(styled, fontFamily = FontFamily.Monospace, fontSize = 12.sp)
            }
        }

        Row(
            modifier = Modifier.horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(6.dp)
        ) {
            listOf("ESC" to "\u001B", "TAB" to "\t", "CTRL+C" to "\u0003", "UP" to "\u001B[A", "DOWN" to "\u001B[B")
                .forEach { (label, seq) ->
                    OutlinedButton(onClick = { store.sendTerminalKeys(seq) }) {
                        Text(label, fontSize = 11.sp)
                    }
                }
        }

        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(
                value = cmd,
                onValueChange = { cmd = it },
                placeholder = { Text("Shell command...") },
                singleLine = true,
                modifier = Modifier.weight(1f)
            )
            Button(onClick = {
                store.sendTerminalKeys("$cmd\r")
                cmd = ""
            }) {
                Text("Run")
            }
        }
    }
}
