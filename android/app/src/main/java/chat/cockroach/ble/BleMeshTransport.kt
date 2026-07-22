@file:Suppress("DEPRECATION")

package chat.cockroach.ble

import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.ParcelUuid
import android.os.PowerManager
import android.util.Log
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong
import uniffi.meshcore_ffi.BleTransport

/**
 * The real BLE transport: every device runs as **both** a GATT server (peripheral, advertising the
 * mesh service) and a scanner/client (central), so any two phones can link up regardless of who
 * saw whom first. Each connection becomes a `link` the core addresses by id.
 *
 * NOTE: this cannot run on an emulator (no Bluetooth radio) — it is validated on physical phones.
 * The in-app loopback ([chat.cockroach.MeshController]) is the emulator stand-in. Structurally this
 * mirrors `docs/IMPLEMENTATION_PLAN.md` §M1: one service UUID, RX write / TX notify, RSSI gate,
 * status-133 close+retry, ≤5 concurrent links.
 */
@SuppressLint("MissingPermission")
class BleMeshTransport(
    private val context: Context,
    private val onLinkUp: (link: ULong, mtu: UInt) -> Unit,
    private val onLinkDown: (link: ULong) -> Unit,
    private val onFrame: (link: ULong, frame: ByteArray) -> Unit,
    private val onStatus: (String) -> Unit = {},
) : BleTransport {

    private val tag = "BleMeshTransport"

    private fun status(msg: String) {
        Log.i(tag, msg)
        onStatus(msg)
    }
    private val manager = context.getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
    private val adapter get() = manager.adapter

    private val nextLink = AtomicLong(1)

    /** A live connection, in either role. */
    private sealed interface Endpoint {
        class Central(val gatt: BluetoothGatt, val rx: BluetoothGattCharacteristic) : Endpoint
        class Peripheral(val device: BluetoothDevice) : Endpoint
    }

    private val links = ConcurrentHashMap<Long, Endpoint>()
    private val retryCount = ConcurrentHashMap<String, Int>()

    private var gattServer: BluetoothGattServer? = null
    private var txChar: BluetoothGattCharacteristic? = null

    // --- lifecycle ------------------------------------------------------------------------------

    fun start() {
        val a = adapter
        if (a == null || !a.isEnabled) {
            status("Bluetooth is OFF — enable it and restart")
            return
        }
        val pm = context.getSystemService(Context.POWER_SERVICE) as PowerManager
        lowPower = !pm.isInteractive
        context.registerReceiver(
            screenReceiver,
            IntentFilter().apply {
                addAction(Intent.ACTION_SCREEN_ON)
                addAction(Intent.ACTION_SCREEN_OFF)
            },
        )
        status("Bluetooth ready; starting mesh (service ${BleConstants.SERVICE_UUID})")
        startGattServer()
        startAdvertising()
        startScanning()
    }

    fun stop() {
        runCatching { context.unregisterReceiver(screenReceiver) }
        adapter?.bluetoothLeScanner?.stopScan(scanCallback)
        adapter?.bluetoothLeAdvertiser?.stopAdvertising(advertiseCallback)
        links.values.forEach { if (it is Endpoint.Central) it.gatt.close() }
        links.clear()
        gattServer?.close()
        gattServer = null
    }

    // --- battery duty-cycling -------------------------------------------------------------------
    // Screen on → aggressive discovery (low-latency scan, balanced advertising). Screen off → back
    // off to low-power modes: the mesh keeps relaying while idle-in-pocket without draining battery.

    @Volatile
    private var lowPower = false

    private val screenReceiver = object : BroadcastReceiver() {
        override fun onReceive(c: Context?, intent: Intent?) {
            when (intent?.action) {
                Intent.ACTION_SCREEN_ON -> setLowPower(false)
                Intent.ACTION_SCREEN_OFF -> setLowPower(true)
            }
        }
    }

    private fun setLowPower(low: Boolean) {
        if (low == lowPower) return
        lowPower = low
        status(if (low) "screen off — low-power mesh" else "screen on — full-power mesh")
        adapter?.bluetoothLeScanner?.stopScan(scanCallback)
        startScanning()
        adapter?.bluetoothLeAdvertiser?.stopAdvertising(advertiseCallback)
        startAdvertising()
    }

    // --- BleTransport (outbound) ----------------------------------------------------------------

    override fun send(link: ULong, frame: ByteArray) {
        when (val ep = links[link.toLong()]) {
            is Endpoint.Central -> {
                ep.rx.value = frame
                ep.rx.writeType = BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
                ep.gatt.writeCharacteristic(ep.rx)
            }
            is Endpoint.Peripheral -> {
                val tx = txChar ?: return
                tx.value = frame
                gattServer?.notifyCharacteristicChanged(ep.device, tx, false)
            }
            null -> Log.w(tag, "send on unknown link $link")
        }
    }

    /// The core deduped this link to another connection with the same peer — tear it down to save
    /// battery and a connection slot, and briefly avoid reconnecting to that address (anti-thrash).
    override fun close(link: ULong) {
        when (val ep = links.remove(link.toLong())) {
            is Endpoint.Central -> {
                cooldown(ep.gatt.device.address)
                ep.gatt.close()
            }
            is Endpoint.Peripheral -> {
                cooldown(ep.device.address)
                gattServer?.cancelConnection(ep.device)
            }
            null -> {}
        }
        status("closed redundant link $link")
    }

    private val recentlyDeduped = ConcurrentHashMap<String, Long>()

    private fun cooldown(address: String) {
        recentlyDeduped[address] = System.currentTimeMillis() + 30_000
    }

    private fun inCooldown(address: String): Boolean {
        val until = recentlyDeduped[address] ?: return false
        if (System.currentTimeMillis() > until) {
            recentlyDeduped.remove(address)
            return false
        }
        return true
    }

    // --- peripheral role (GATT server + advertising) --------------------------------------------

    private fun startGattServer() {
        val server = manager.openGattServer(context, serverCallback) ?: return
        val service = BluetoothGattService(BleConstants.SERVICE_UUID, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        val rx = BluetoothGattCharacteristic(
            BleConstants.RX_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_WRITE or BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE,
            BluetoothGattCharacteristic.PERMISSION_WRITE,
        )
        val tx = BluetoothGattCharacteristic(
            BleConstants.TX_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            BluetoothGattCharacteristic.PERMISSION_READ,
        ).apply {
            addDescriptor(
                BluetoothGattDescriptor(
                    BleConstants.CCCD_UUID,
                    BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE,
                )
            )
        }
        service.addCharacteristic(rx)
        service.addCharacteristic(tx)
        server.addService(service)
        gattServer = server
        txChar = tx
        status("GATT server open (peripheral role)")
    }

    private val serverCallback = object : BluetoothGattServerCallback() {
        override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
            if (newState == BluetoothProfile.STATE_CONNECTED) {
                val id = nextLink.getAndIncrement()
                links[id] = Endpoint.Peripheral(device)
                status("LINK UP (peripheral) from ${device.address} — link $id")
                onLinkUp(id.toULong(), 182u)
            } else if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                dropLinksForDevice(device)
            }
        }

        override fun onCharacteristicWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            characteristic: BluetoothGattCharacteristic,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray,
        ) {
            if (characteristic.uuid == BleConstants.RX_CHAR_UUID) {
                linkFor(device)?.let { onFrame(it.toULong(), value) }
            }
            if (responseNeeded) {
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
            }
        }

        override fun onDescriptorWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            descriptor: BluetoothGattDescriptor,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray,
        ) {
            if (responseNeeded) {
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
            }
        }
    }

    private fun startAdvertising() {
        val advertiser = adapter?.bluetoothLeAdvertiser ?: return
        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(
                if (lowPower) AdvertiseSettings.ADVERTISE_MODE_LOW_POWER
                else AdvertiseSettings.ADVERTISE_MODE_BALANCED
            )
            .setConnectable(true)
            .setTimeout(0)
            .build()
        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .addServiceUuid(ParcelUuid(BleConstants.SERVICE_UUID))
            .build()
        advertiser.startAdvertising(settings, data, advertiseCallback)
    }

    private val advertiseCallback = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
            status("advertising started — this phone is discoverable")
        }

        override fun onStartFailure(errorCode: Int) {
            status("advertise FAILED: $errorCode")
        }
    }

    // --- central role (scan + connect) ----------------------------------------------------------

    private fun startScanning() {
        val scanner = adapter?.bluetoothLeScanner ?: return
        val filters = listOf(
            ScanFilter.Builder().setServiceUuid(ParcelUuid(BleConstants.SERVICE_UUID)).build(),
        )
        val settings = ScanSettings.Builder()
            .setScanMode(
                if (lowPower) ScanSettings.SCAN_MODE_LOW_POWER
                else ScanSettings.SCAN_MODE_LOW_LATENCY
            )
            .build()
        scanner.startScan(filters, settings, scanCallback)
        status("scanning started (central role, ${if (lowPower) "low-power" else "low-latency"})")
    }

    private val scanCallback = object : ScanCallback() {
        override fun onScanResult(callbackType: Int, result: ScanResult) {
            val device = result.device
            status("saw ${device.address} rssi ${result.rssi}")
            if (result.rssi < BleConstants.RSSI_GATE_DBM) return
            if (links.size >= BleConstants.MAX_LINKS) return
            if (isConnected(device)) return
            if (inCooldown(device.address)) return // recently deduped — don't immediately reconnect
            status("connecting to ${device.address}")
            device.connectGatt(context, false, gattCallback, BluetoothDevice.TRANSPORT_LE)
        }

        override fun onScanFailed(errorCode: Int) {
            status("scan FAILED: $errorCode")
        }
    }

    private val gattCallback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            val addr = gatt.device.address
            if (status == BleConstants.GATT_STATUS_133 || newState == BluetoothProfile.STATE_DISCONNECTED) {
                gatt.close() // always close, never just disconnect (status-133 discipline)
                dropLinksForDevice(gatt.device)
                if (status == BleConstants.GATT_STATUS_133) {
                    val n = (retryCount[addr] ?: 0) + 1
                    retryCount[addr] = n
                    if (n <= 5) gatt.device.connectGatt(context, false, this, BluetoothDevice.TRANSPORT_LE)
                }
                return
            }
            if (newState == BluetoothProfile.STATE_CONNECTED) {
                retryCount.remove(addr)
                gatt.requestMtu(517)
            }
        }

        override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
            gatt.discoverServices()
        }

        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            val service = gatt.getService(BleConstants.SERVICE_UUID) ?: return
            val rx = service.getCharacteristic(BleConstants.RX_CHAR_UUID) ?: return
            val tx = service.getCharacteristic(BleConstants.TX_CHAR_UUID) ?: return
            gatt.setCharacteristicNotification(tx, true)
            tx.getDescriptor(BleConstants.CCCD_UUID)?.let { cccd ->
                cccd.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
                gatt.writeDescriptor(cccd)
            }
            val id = nextLink.getAndIncrement()
            links[id] = Endpoint.Central(gatt, rx)
            status("LINK UP (central) to ${gatt.device.address} — link $id")
            // MTU-3 is the usable ATT payload; clamp to the core's 182 floor assumption.
            onLinkUp(id.toULong(), 182u)
        }

        override fun onCharacteristicChanged(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray,
        ) {
            if (characteristic.uuid == BleConstants.TX_CHAR_UUID) {
                linkFor(gatt.device)?.let { onFrame(it.toULong(), value) }
            }
        }
    }

    // --- helpers --------------------------------------------------------------------------------

    private fun isConnected(device: BluetoothDevice): Boolean =
        links.values.any { deviceOf(it)?.address == device.address }

    private fun linkFor(device: BluetoothDevice): Long? =
        links.entries.firstOrNull { deviceOf(it.value)?.address == device.address }?.key

    private fun dropLinksForDevice(device: BluetoothDevice) {
        val ids = links.entries.filter { deviceOf(it.value)?.address == device.address }.map { it.key }
        ids.forEach { id ->
            links.remove(id)
            onLinkDown(id.toULong())
        }
    }

    private fun deviceOf(ep: Endpoint): BluetoothDevice? = when (ep) {
        is Endpoint.Central -> ep.gatt.device
        is Endpoint.Peripheral -> ep.device
    }
}
