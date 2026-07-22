package chat.cockroach.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertSame
import org.junit.Test

/**
 * Guards the i18n catalog. The `Strings` class takes every field as a constructor parameter, so a
 * *missing* translation is impossible by construction — but a translation that drops or reorders a
 * `String.format` specifier compiles fine and then throws at runtime, in the cooldown and composer
 * paths where users are already being told to slow down.
 */
class StringsTest {

    /** Public no-arg String getters on [Strings] — one per catalog entry. */
    private val entries: List<java.lang.reflect.Method> =
        Strings::class.java.methods
            .filter { it.name.startsWith("get") && it.parameterCount == 0 && it.returnType == String::class.java }
            .sortedBy { it.name }

    /** Conversion characters in declaration order, ignoring the literal `%%` escape. */
    private fun specifiers(s: String): List<Char> =
        Regex("""%(?:\d+\$)?[-#+ 0,(]*\d*(?:\.\d+)?([a-zA-Z%])""")
            .findAll(s)
            .map { it.groupValues[1].first() }
            .filter { it != '%' }
            .toList()

    @Test
    fun `catalog is non-trivial`() {
        // Cheap canary: if reflection stops finding the getters, the tests below would all vacuously
        // pass. The catalog is ~170 entries; any collapse means the lookup broke.
        assert(entries.size > 50) { "expected the full catalog, found ${entries.size} entries" }
    }

    @Test
    fun `hindi keeps every format specifier english declares`() {
        val mismatches = entries.mapNotNull { getter ->
            val en = getter.invoke(EnStrings) as String
            val hi = getter.invoke(HiStrings) as String
            val enSpec = specifiers(en)
            val hiSpec = specifiers(hi)
            if (enSpec == hiSpec) null else "${getter.name}: en=$enSpec hi=$hiSpec\n  en=\"$en\"\n  hi=\"$hi\""
        }
        assertEquals("format specifiers drifted between en and hi:\n" + mismatches.joinToString("\n"), 0, mismatches.size)
    }

    @Test
    fun `no catalog entry is blank`() {
        for (getter in entries) {
            for ((lang, catalog) in listOf("en" to EnStrings, "hi" to HiStrings)) {
                val v = getter.invoke(catalog) as String
                assertFalse("$lang.${getter.name} is blank", v.isBlank())
            }
        }
    }

    @Test
    fun `stringsFor resolves known codes and falls back to english`() {
        assertSame(HiStrings, stringsFor("hi"))
        assertSame(EnStrings, stringsFor("en"))
        assertSame("an unknown locale must not crash the first-run picker", EnStrings, stringsFor("xx"))
    }

    @Test
    fun `Lang fromCode falls back to english`() {
        assertEquals(Lang.HI, Lang.fromCode("hi"))
        assertEquals(Lang.EN, Lang.fromCode("en"))
        assertEquals(Lang.EN, Lang.fromCode(""))
        assertEquals(Lang.EN, Lang.fromCode("nope"))
    }
}
