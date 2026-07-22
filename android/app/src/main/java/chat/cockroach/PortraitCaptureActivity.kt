package chat.cockroach

import com.journeyapps.barcodescanner.CaptureActivity

/**
 * ZXing capture activity locked to portrait so the QR scanner is vertical with a square viewfinder,
 * instead of the library's default landscape (horizontal) framing.
 */
class PortraitCaptureActivity : CaptureActivity()
