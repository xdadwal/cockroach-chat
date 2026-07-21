@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.cockroach

import android.Manifest
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.lifecycleScope

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val loopback = MeshController(lifecycleScope).also { it.start() }
        val ble = BleController(this, lifecycleScope)
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    App(loopback, ble)
                }
            }
        }
    }
}

private enum class Mode { BLE, LOOPBACK }

@Composable
private fun App(loopback: MeshController, ble: BleController) {
    var mode by remember { mutableStateOf(Mode.BLE) }
    Column(Modifier.fillMaxSize().padding(12.dp)) {
        Text("Cockroach Chat", fontSize = 22.sp, color = MaterialTheme.colorScheme.primary)
        SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
            SegmentedButton(
                selected = mode == Mode.BLE,
                onClick = { mode = Mode.BLE },
                shape = SegmentedButtonDefaults.itemShape(0, 2),
            ) { Text("Real BLE") }
            SegmentedButton(
                selected = mode == Mode.LOOPBACK,
                onClick = { mode = Mode.LOOPBACK },
                shape = SegmentedButtonDefaults.itemShape(1, 2),
            ) { Text("Loopback demo") }
        }
        when (mode) {
            Mode.BLE -> BleScreen(ble)
            Mode.LOOPBACK -> LoopbackScreen(loopback)
        }
    }
}

// --- Real BLE ------------------------------------------------------------------------------------

@Composable
private fun BleScreen(ble: BleController) {
    val permissions = remember {
        buildList {
            if (Build.VERSION.SDK_INT >= 31) {
                add(Manifest.permission.BLUETOOTH_SCAN)
                add(Manifest.permission.BLUETOOTH_CONNECT)
                add(Manifest.permission.BLUETOOTH_ADVERTISE)
            }
            if (Build.VERSION.SDK_INT >= 33) add(Manifest.permission.POST_NOTIFICATIONS)
        }.toTypedArray()
    }
    val launcher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { result ->
        val granted = result.filterKeys { it != Manifest.permission.POST_NOTIFICATIONS }.all { it.value }
        if (granted) ble.startBle() else ble.log.add("BLE permissions denied")
    }

    Column(Modifier.fillMaxSize()) {
        Text(
            "Runs the real Bluetooth-LE transport (advertise + scan + GATT). " +
                "With a second phone running this app, they mesh over BLE — no internet.",
            fontSize = 12.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(6.dp))
        if (!ble.running.value) {
            Button(onClick = { launcher.launch(permissions) }, modifier = Modifier.fillMaxWidth()) {
                Text("Start BLE mesh")
            }
        } else {
            Text("● live — eph ${ble.ephId.value.take(8)}", color = MaterialTheme.colorScheme.primary)
        }

        Card(Modifier.fillMaxWidth().weight(1f).padding(top = 8.dp)) {
            Column(Modifier.padding(8.dp)) {
                Text("📱 ${Build.MODEL} · #general", fontSize = 14.sp, color = MaterialTheme.colorScheme.primary)
                LazyColumn(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    items(ble.messages) { Bubble(it) }
                }
                var draft by remember { mutableStateOf("") }
                OutlinedTextField(
                    value = draft,
                    onValueChange = { draft = it },
                    placeholder = { Text("message…") },
                    singleLine = true,
                    enabled = ble.running.value,
                    modifier = Modifier.fillMaxWidth(),
                )
                Button(
                    onClick = { ble.send(draft); draft = "" },
                    enabled = ble.running.value,
                    modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
                ) { Text("Send") }
            }
        }
        StatusStrip("BLE log", ble.log)
    }
}

// --- Loopback demo -------------------------------------------------------------------------------

@Composable
private fun LoopbackScreen(mesh: MeshController) {
    Column(Modifier.fillMaxSize()) {
        Text(
            "Two mesh nodes (Rust core) chatting over a loopback stand-in for BLE",
            fontSize = 12.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(8.dp))
        Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            PhonePane("Ava", mesh.phoneA, Modifier.weight(1f)) { mesh.send(0, it) }
            PhonePane("Ben", mesh.phoneB, Modifier.weight(1f)) { mesh.send(1, it) }
        }
        StatusStrip("mesh log", mesh.status)
    }
}

@Composable
private fun PhonePane(
    name: String,
    messages: List<ChatMessage>,
    modifier: Modifier,
    onSend: (String) -> Unit,
) {
    Card(modifier.fillMaxHeight()) {
        Column(Modifier.padding(8.dp)) {
            Text("📱 $name · #general", fontSize = 14.sp, color = MaterialTheme.colorScheme.primary)
            Spacer(Modifier.height(4.dp))
            LazyColumn(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                items(messages) { Bubble(it) }
            }
            var draft by remember { mutableStateOf("") }
            OutlinedTextField(
                value = draft,
                onValueChange = { draft = it },
                placeholder = { Text("message…") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Button(
                onClick = { onSend(draft); draft = "" },
                modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
            ) { Text("Send") }
        }
    }
}

// --- shared --------------------------------------------------------------------------------------

@Composable
private fun Bubble(m: ChatMessage) {
    val align = if (m.mine) Alignment.End else Alignment.Start
    val bg = if (m.mine) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceVariant
    Column(Modifier.fillMaxWidth(), horizontalAlignment = align) {
        Box(Modifier.background(bg, RoundedCornerShape(10.dp)).padding(horizontal = 10.dp, vertical = 6.dp)) {
            Text(m.body, fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurface)
        }
        if (!m.mine) {
            Text(
                if (m.verified) "✓ verified" else "unverified",
                fontSize = 9.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun StatusStrip(label: String, status: List<String>) {
    Spacer(Modifier.height(6.dp))
    Text(label, fontSize = 11.sp, color = MaterialTheme.colorScheme.primary)
    Column(
        Modifier
            .fillMaxWidth()
            .height(110.dp)
            .background(Color(0x22000000), RoundedCornerShape(6.dp))
            .padding(6.dp)
            .verticalScroll(rememberScrollState())
    ) {
        status.takeLast(12).forEach {
            Text("· $it", fontSize = 10.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}
