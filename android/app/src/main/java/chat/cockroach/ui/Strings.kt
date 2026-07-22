package chat.cockroach.ui

import androidx.compose.runtime.staticCompositionLocalOf

/** Supported UI languages. `code` is what's persisted; `display` is the native name shown to users. */
enum class Lang(val code: String, val display: String) {
    EN("en", "English"),
    HI("hi", "हिन्दी");

    companion object {
        fun fromCode(c: String): Lang = entries.firstOrNull { it.code == c } ?: EN
    }
}

/** Every user-facing UI string. Dynamic content (messages, names, fingerprints, channel ids) is
 *  never translated. Strings with %s/%d are filled with `String.format`. */
class Strings(
    // language picker
    val langTitle: String,
    val langSubtitle: String,
    val langContinue: String,
    // onboarding name
    val onbTagline: String,
    val onbNameLabel: String,
    val onbNamePlaceholder: String,
    val onbNameWarn: String,
    val onbEnter: String,
    // mesh chip
    val meshLive: String,
    val meshScanning: String,
    val meshLowPower: String,
    val meshOff: String,
    // mesh-off screen
    val offTitle: String,
    val offBody: String,
    val offHint: String,
    val offStart: String,
    // feed tabs
    val tabAnnouncement: String,
    val tabChannels: String,
    val tabVerified: String,
    // bottom nav
    val navFeed: String,
    val navMe: String,
    // banners
    val publicBanner: String,
    val e2eBanner: String,
    // composer
    val composerAnnounce: String,
    val composerChannel: String,
    val composerDm: String,
    val announceRate: String,
    // cooldown
    val coolTitle: String,
    val coolAnnounceSub: String,
    val slowTitle: String,
    val slowSub: String,
    // channels tab
    val publicChannelsLabel: String,
    val channelsFooter: String,
    val channelQuiet: String,
    val someone: String,
    // verified tab
    val dmTabBanner: String,
    val dmEmpty: String,
    // channel screen
    val channelPublicOwnerless: String,
    // dm header
    val dmVerifiedSubtitle: String,
    val dmNotVerified: String,
    // peer row subtitles
    val peerYourPetname: String,
    val peerClaims: String,
    // message bubble
    val badgeVerified: String,
    val badgeUnverified: String,
    val msgYouSigned: String,
    val msgCantConfirm: String,
    val unknown: String,
    // verify flow
    val verifyTitle: String,
    val verifyMismatchTitle: String,
    val verifyShowHeading: String,
    val verifyShowBody: String,
    val safetyNumber: String,
    val scanTheirs: String,
    val scanPrompt: String,
    val matchTitle: String,
    val matchBody: String,
    val petnameLabel: String,
    val petnamePlaceholder: String,
    val saveOpenDm: String,
    val mismatchBody: String,
    val tryScanAgain: String,
    val cancelUnverified: String,
    // identity / me
    val identityTitle: String,
    val showMyQr: String,
    val showMyQrSub: String,
    val scanQr: String,
    val scanQrSub: String,
    val languageLabel: String,
    val dangerZone: String,
    val panicWipe: String,
    val panicBody: String,
    val holdToWipe: String,
    val keepHolding: String,
    // status
    val statusTitle: String,
    val statusSubtitle: String,
    val statPhones: String,
    val statCarried: String,
    val statVerified: String,
    val batteryAware: String,
    val recentActivity: String,
    // credits — section headings and prose are translated; the credited names, authors and
    // licence identifiers are proper nouns and stay as their owners write them.
    val creditsFooter: String,
    val creditsFooterSub: String,
    val creditsTitle: String,
    val creditsSubtitle: String,
    val creditsIntro: String,
    val creditsType: String,
    val creditsCrypto: String,
    val creditsCore: String,
    val creditsAndroid: String,
    val creditsTooling: String,
    val creditsThanks: String,
    val creditsFull: String,
    // channel display names (keyed by the English wire id, which never changes)
    val channelNames: Map<String, String>,
)

val EnStrings = Strings(
    langTitle = "Choose your language",
    langSubtitle = "You can change this later on the Me page.",
    langContinue = "Continue",
    onbTagline = "No sign-up. No servers. Pick a name people nearby will see — you can change it anytime.",
    onbNameLabel = "Display name · shown on the wire",
    onbNamePlaceholder = "your name",
    onbNameWarn = "Names are not identity. Anyone can use any name until you verify them in person.",
    onbEnter = "Enter →",
    meshLive = "LIVE",
    meshScanning = "SCANNING",
    meshLowPower = "LOW-POWER",
    meshOff = "MESH OFF",
    offTitle = "You're not on the mesh yet.",
    offBody = "Tap to switch your radio on. While it's on, you can send messages and carry other people's.",
    offHint = "Tap the button to go live ↑",
    offStart = "Start mesh",
    tabAnnouncement = "Announcement",
    tabChannels = "Channels",
    tabVerified = "Verified",
    navFeed = "Feed",
    navMe = "Me",
    publicBanner = "Public broadcast. Everyone in range reads this — including police.",
    e2eBanner = "Encrypted to this device. Only you two can read it — but metadata still travels, and a seized phone still holds this thread.",
    composerAnnounce = "Broadcast to everyone in range…",
    composerChannel = "Message %s…",
    composerDm = "Encrypted message…",
    announceRate = "Announcements are rate-limited to 1 per minute.",
    coolTitle = "Cooling down",
    coolAnnounceSub = "Broadcast again in %ds. Keeps the square readable.",
    slowTitle = "Slow down",
    slowSub = "2 messages per 10s. Try again in %ds.",
    publicChannelsLabel = "Public channels",
    channelsFooter = "Public channels — anyone in range reads them. No lock, ever.",
    channelQuiet = "quiet · no messages yet",
    someone = "someone",
    dmTabBanner = "Your encrypted DMs — end-to-end encrypted to each device.",
    dmEmpty = "Verify people in person to build a private, spoof-proof circle.",
    channelPublicOwnerless = "public · ownerless",
    dmVerifiedSubtitle = "verified · your petname",
    dmNotVerified = "not verified",
    peerYourPetname = "your petname · in range",
    peerClaims = "claims this name · in range",
    badgeVerified = "VERIFIED",
    badgeUnverified = "UNVERIFIED",
    msgYouSigned = "you · signed ✓",
    msgCantConfirm = "can't confirm sender",
    unknown = "unknown",
    verifyTitle = "Verify in person",
    verifyMismatchTitle = "These don't match",
    verifyShowHeading = "Show this to your friend",
    verifyShowBody = "When they scan it, you're added to their verified circle. Do this face-to-face only.",
    safetyNumber = "Safety number",
    scanTheirs = "Scan theirs instead →",
    scanPrompt = "Point at your friend's screen",
    matchTitle = "Fingerprints match.",
    matchBody = "This key is now verified. Their messages will carry the green shield. Give them a private name only you'll see.",
    petnameLabel = "Private petname",
    petnamePlaceholder = "their name",
    saveOpenDm = "Save & open DM",
    mismatchBody = "This is not the key you scanned — someone may be impersonating them, or you scanned the wrong screen.",
    tryScanAgain = "Try scanning again",
    cancelUnverified = "Cancel — stay unverified",
    identityTitle = "Identity",
    showMyQr = "Show my QR",
    showMyQrSub = "Add me to someone's verified circle",
    scanQr = "Scan a QR",
    scanQrSub = "Verify someone by scanning theirs",
    languageLabel = "Language",
    dangerZone = "Danger zone",
    panicWipe = "Panic wipe",
    panicBody = "Press and hold for 5 seconds to erase your keys and every message — for when your phone is about to be seized. This cannot be undone.",
    holdToWipe = "Hold to wipe · 5s",
    keepHolding = "Keep holding… %d%%",
    statusTitle = "Mesh status",
    statusSubtitle = "You're carrying the network right now.",
    statPhones = "phones in range",
    statCarried = "messages carried",
    statVerified = "verified nearby",
    batteryAware = "Battery-aware. Drops to low-power relay when the screen is off or battery is low.",
    recentActivity = "Recent activity",
    creditsFooter = "Credits",
    creditsFooterSub = "The work this app is built on",
    creditsTitle = "Credits",
    creditsSubtitle = "open source",
    creditsIntro = "Cockroach Chat is assembled almost entirely from work other people gave away. " +
        "This page names them.",
    creditsType = "Type",
    creditsCrypto = "Cryptography",
    creditsCore = "Core & bindings",
    creditsAndroid = "App",
    creditsTooling = "Tooling",
    creditsThanks = "We hand-roll no cryptography. Whatever safety this app offers, these people " +
        "built it — the mistakes are ours alone.",
    creditsFull = "Font licences ship inside this app. The full dependency list is in NOTICE.md " +
        "in the source repository.",
    channelNames = mapOf(
        "general" to "general", "alerts" to "alerts", "medics" to "medics",
        "supplies" to "supplies", "lost+found" to "lost+found", "exits" to "exits",
    ),
)

val HiStrings = Strings(
    langTitle = "अपनी भाषा चुनें",
    langSubtitle = "आप इसे बाद में 'Me' पेज पर बदल सकते हैं।",
    langContinue = "जारी रखें",
    onbTagline = "कोई साइन-अप नहीं। कोई सर्वर नहीं। एक नाम चुनें जो आस-पास के लोग देखेंगे — इसे कभी भी बदला जा सकता है।",
    onbNameLabel = "प्रदर्शित नाम · नेटवर्क पर दिखेगा",
    onbNamePlaceholder = "आपका नाम",
    onbNameWarn = "नाम पहचान नहीं है। जब तक आप किसी को आमने-सामने सत्यापित न करें, कोई भी कोई भी नाम इस्तेमाल कर सकता है।",
    onbEnter = "प्रवेश करें →",
    meshLive = "लाइव",
    meshScanning = "स्कैन जारी",
    meshLowPower = "लो-पावर",
    meshOff = "मेश बंद",
    offTitle = "आप अभी मेश पर नहीं हैं।",
    offBody = "अपना रेडियो चालू करने के लिए टैप करें। चालू रहने पर आप संदेश भेज सकते हैं और दूसरों के संदेश आगे पहुँचा सकते हैं।",
    offHint = "लाइव होने के लिए बटन दबाएँ ↑",
    offStart = "मेश शुरू करें",
    tabAnnouncement = "घोषणा",
    tabChannels = "चैनल",
    tabVerified = "वेरिफाइड",
    navFeed = "सूचना",
    navMe = "मैं",
    publicBanner = "सार्वजनिक प्रसारण। रेंज में मौजूद हर कोई इसे पढ़ सकता है — पुलिस सहित।",
    e2eBanner = "इस डिवाइस के लिए एन्क्रिप्टेड। केवल आप दोनों इसे पढ़ सकते हैं — पर मेटाडेटा फिर भी यात्रा करता है, और ज़ब्त फ़ोन इस बातचीत को अपने पास रखता है।",
    composerAnnounce = "रेंज में सभी को प्रसारित करें…",
    composerChannel = "%s में संदेश…",
    composerDm = "एन्क्रिप्टेड संदेश…",
    announceRate = "घोषणाएँ प्रति मिनट 1 तक सीमित हैं।",
    coolTitle = "थोड़ा रुकें",
    coolAnnounceSub = "%d सेकंड में फिर प्रसारित करें। इससे चौक पढ़ने योग्य रहता है।",
    slowTitle = "धीरे करें",
    slowSub = "प्रति 10 सेकंड 2 संदेश। %d सेकंड में पुनः प्रयास करें।",
    publicChannelsLabel = "सार्वजनिक चैनल",
    channelsFooter = "सार्वजनिक चैनल — रेंज में मौजूद कोई भी इन्हें पढ़ सकता है। कभी कोई ताला नहीं।",
    channelQuiet = "शांत · अभी कोई संदेश नहीं",
    someone = "कोई",
    dmTabBanner = "आपके एन्क्रिप्टेड DM — प्रत्येक डिवाइस के लिए एंड-टू-एंड एन्क्रिप्टेड।",
    dmEmpty = "एक निजी, सुरक्षित मंडली बनाने के लिए लोगों को आमने-सामने सत्यापित करें।",
    channelPublicOwnerless = "सार्वजनिक · बिना स्वामी",
    dmVerifiedSubtitle = "सत्यापित · आपका उपनाम",
    dmNotVerified = "सत्यापित नहीं",
    peerYourPetname = "आपका उपनाम · रेंज में",
    peerClaims = "यह नाम बताता है · रेंज में",
    badgeVerified = "सत्यापित",
    badgeUnverified = "असत्यापित",
    msgYouSigned = "आप · हस्ताक्षरित ✓",
    msgCantConfirm = "प्रेषक की पुष्टि नहीं",
    unknown = "अज्ञात",
    verifyTitle = "आमने-सामने सत्यापित करें",
    verifyMismatchTitle = "ये मेल नहीं खाते",
    verifyShowHeading = "इसे अपने मित्र को दिखाएँ",
    verifyShowBody = "जब वे इसे स्कैन करेंगे, आप उनकी सत्यापित मंडली में जुड़ जाएँगे। यह केवल आमने-सामने करें।",
    safetyNumber = "सुरक्षा संख्या",
    scanTheirs = "इसके बजाय उनका स्कैन करें →",
    scanPrompt = "अपने मित्र की स्क्रीन पर कैमरा रखें",
    matchTitle = "फ़िंगरप्रिंट मेल खाते हैं।",
    matchBody = "यह कुंजी अब सत्यापित है। उनके संदेशों पर हरा ढाल दिखेगा। उन्हें एक निजी नाम दें जो केवल आप देखेंगे।",
    petnameLabel = "निजी उपनाम",
    petnamePlaceholder = "उनका नाम",
    saveOpenDm = "सहेजें और DM खोलें",
    mismatchBody = "यह वह कुंजी नहीं है जो आपने स्कैन की — कोई उनका रूप धर सकता है, या आपने ग़लत स्क्रीन स्कैन की।",
    tryScanAgain = "फिर से स्कैन करें",
    cancelUnverified = "रद्द करें — असत्यापित रहें",
    identityTitle = "पहचान",
    showMyQr = "मेरा QR दिखाएँ",
    showMyQrSub = "मुझे किसी की सत्यापित मंडली में जोड़ें",
    scanQr = "QR स्कैन करें",
    scanQrSub = "किसी का QR स्कैन कर उसे सत्यापित करें",
    languageLabel = "भाषा",
    dangerZone = "ख़तरा क्षेत्र",
    panicWipe = "आपातकालीन मिटाना",
    panicBody = "अपनी कुंजियाँ और हर संदेश मिटाने के लिए 5 सेकंड दबाकर रखें — जब आपका फ़ोन ज़ब्त होने वाला हो। इसे पूर्ववत नहीं किया जा सकता।",
    holdToWipe = "मिटाने के लिए दबाए रखें · 5से",
    keepHolding = "दबाए रखें… %d%%",
    statusTitle = "मेश स्थिति",
    statusSubtitle = "आप अभी नेटवर्क को आगे पहुँचा रहे हैं।",
    statPhones = "रेंज में फ़ोन",
    statCarried = "पहुँचाए संदेश",
    statVerified = "पास में सत्यापित",
    batteryAware = "बैटरी के प्रति सजग। स्क्रीन बंद या बैटरी कम होने पर लो-पावर रिले पर चला जाता है।",
    recentActivity = "हाल की गतिविधि",
    creditsFooter = "श्रेय",
    creditsFooterSub = "जिनके काम पर यह ऐप बना है",
    creditsTitle = "श्रेय",
    creditsSubtitle = "ओपन सोर्स",
    creditsIntro = "कॉकरोच चैट लगभग पूरी तरह उस काम से बना है जो दूसरे लोगों ने मुफ़्त में साझा किया। " +
        "यह पन्ना उनके नाम दर्ज करता है।",
    creditsType = "टाइपफ़ेस",
    creditsCrypto = "क्रिप्टोग्राफ़ी",
    creditsCore = "कोर और बाइंडिंग",
    creditsAndroid = "ऐप",
    creditsTooling = "उपकरण",
    creditsThanks = "हम कोई क्रिप्टोग्राफ़ी खुद नहीं लिखते। यह ऐप जितना भी सुरक्षित है, वह इन्हीं लोगों " +
        "का बनाया हुआ है — ग़लतियाँ सिर्फ़ हमारी हैं।",
    creditsFull = "फ़ॉन्ट लाइसेंस इसी ऐप के भीतर मौजूद हैं। पूरी सूची सोर्स रिपॉज़िटरी की NOTICE.md में है।",
    channelNames = mapOf(
        "general" to "सामान्य", "alerts" to "चेतावनी", "medics" to "चिकित्सा",
        "supplies" to "आपूर्ति", "lost+found" to "खोया-पाया", "exits" to "निकास",
    ),
)

fun stringsFor(code: String): Strings = if (code == "hi") HiStrings else EnStrings

/** Localized display name for a channel; the wire id (English) is left unchanged. */
fun Strings.channelLabel(id: String): String {
    val key = id.removePrefix("#")
    return channelNames[key] ?: key
}

/** The active string catalog for the composition subtree. */
val LocalStrings = staticCompositionLocalOf { EnStrings }

/** Text scale factor — bumped for Devanagari, which reads small at Latin point sizes. */
val LocalFontScale = staticCompositionLocalOf { 1f }

fun Strings.meshLabel(state: MeshState): String = when (state) {
    MeshState.Live -> meshLive
    MeshState.Scanning -> meshScanning
    MeshState.LowPower -> meshLowPower
    MeshState.Off -> meshOff
}

fun Strings.feedLabel(tab: FeedTab): String = when (tab) {
    FeedTab.Announce -> tabAnnouncement
    FeedTab.Nearby -> tabChannels
    FeedTab.Verified -> tabVerified
}
