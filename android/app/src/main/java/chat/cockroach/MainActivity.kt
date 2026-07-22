package chat.cockroach

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import androidx.core.view.WindowCompat
import chat.cockroach.ui.CcBase
import chat.cockroach.ui.CockroachApp
import chat.cockroach.ui.CockroachTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Block screenshots + hide from the recents thumbnail (shoulder-surf / seizure defense).
        window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
        // Draw edge-to-edge so we can inset content ourselves (keeps the composer above the keyboard).
        WindowCompat.setDecorFitsSystemWindows(window, false)
        // Shared, process-lifetime controller — the foreground service keeps its mesh alive when we're
        // backgrounded; this Activity only observes/commands it.
        val ble = BleController.get(this)
        setContent {
            CockroachTheme {
                Surface(Modifier.fillMaxSize(), color = CcBase) {
                    // safeDrawing = status/nav bars + IME: lifts the whole UI above the soft keyboard.
                    Box(Modifier.fillMaxSize().safeDrawingPadding()) {
                        CockroachApp(ble)
                    }
                }
            }
        }
    }
}
