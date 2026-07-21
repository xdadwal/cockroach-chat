package chat.cockroach

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.lifecycleScope

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val mesh = MeshController(lifecycleScope)
        mesh.start()
        setContent {
            MaterialTheme(colorScheme = darkColorScheme()) {
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    DemoScreen(mesh)
                }
            }
        }
    }
}

@Composable
private fun DemoScreen(mesh: MeshController) {
    Column(Modifier.fillMaxSize().padding(12.dp)) {
        Text("Cockroach Chat", fontSize = 22.sp, color = MaterialTheme.colorScheme.primary)
        Text(
            "Two real mesh nodes (Rust core) chatting over a loopback stand-in for BLE",
            fontSize = 12.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(8.dp))
        Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            PhonePane("Ava", mesh.phoneA, Modifier.weight(1f)) { mesh.send(0, it) }
            PhonePane("Ben", mesh.phoneB, Modifier.weight(1f)) { mesh.send(1, it) }
        }
        StatusStrip(mesh.status)
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
            Text("📱 $name  ·  #general", fontSize = 14.sp, color = MaterialTheme.colorScheme.primary)
            Spacer(Modifier.height(4.dp))
            LazyColumn(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                items(messages) { m -> Bubble(m) }
            }
            var draft by remember { mutableStateOf("") }
            OutlinedTextField(
                value = draft,
                onValueChange = { draft = it },
                placeholder = { Text("message…") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
                keyboardActions = KeyboardActions(onSend = {
                    onSend(draft); draft = ""
                }),
            )
            Button(
                onClick = { onSend(draft); draft = "" },
                modifier = Modifier.fillMaxWidth().padding(top = 4.dp),
            ) { Text("Send") }
        }
    }
}

@Composable
private fun Bubble(m: ChatMessage) {
    val align = if (m.mine) Alignment.End else Alignment.Start
    val bg = if (m.mine) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceVariant
    Column(Modifier.fillMaxWidth(), horizontalAlignment = align) {
        Box(
            Modifier.background(bg, RoundedCornerShape(10.dp)).padding(horizontal = 10.dp, vertical = 6.dp)
        ) {
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
private fun StatusStrip(status: List<String>) {
    Spacer(Modifier.height(6.dp))
    Text("mesh log", fontSize = 11.sp, color = MaterialTheme.colorScheme.primary)
    Column(
        Modifier
            .fillMaxWidth()
            .height(72.dp)
            .background(Color(0x22000000), RoundedCornerShape(6.dp))
            .padding(6.dp)
            .verticalScroll(rememberScrollState())
    ) {
        status.takeLast(6).forEach {
            Text("· $it", fontSize = 10.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}
