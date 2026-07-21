package chat.cockroach.ble

import java.util.UUID

/**
 * BLE GATT identifiers for the mesh. One service; a write characteristic (central → peripheral)
 * and a notify characteristic (peripheral → central). Every node runs both roles so any two
 * phones can connect regardless of who discovers whom.
 */
object BleConstants {
    val SERVICE_UUID: UUID = UUID.fromString("c0c04a17-0000-1000-8000-00805f9b34fb")
    /** Central writes inbound frames here (write-without-response for throughput). */
    val RX_CHAR_UUID: UUID = UUID.fromString("c0c04a17-0001-1000-8000-00805f9b34fb")
    /** Peripheral pushes outbound frames here via notifications. */
    val TX_CHAR_UUID: UUID = UUID.fromString("c0c04a17-0002-1000-8000-00805f9b34fb")
    /** Standard Client Characteristic Configuration Descriptor. */
    val CCCD_UUID: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")

    const val RSSI_GATE_DBM = -85
    const val MAX_LINKS = 5
    const val GATT_STATUS_133 = 133
}
