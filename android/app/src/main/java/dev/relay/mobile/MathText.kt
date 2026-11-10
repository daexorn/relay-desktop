package dev.relay.mobile

/**
 * Brings every formula to the forms Markwon's LaTeX plugin reads: `$$…$$` inside a line and a
 * `$$` block on lines of its own. Same rules as the desktop `math::prepare`: code stays code,
 * prices like `$5` stay text, and environments written without dollars become blocks.
 */
object MathText {
    private val ENVIRONMENTS = setOf(
        "equation", "equation*", "align", "align*", "aligned", "gather", "gather*", "gathered", "multline",
        "multline*", "eqnarray", "eqnarray*", "split", "cases", "matrix", "pmatrix", "bmatrix", "Bmatrix",
        "vmatrix", "Vmatrix", "array", "alignat",
    )

    /** A formula is still being written: it is hidden and this mark shows in its place. */
    const val PENDING = " ⋯"

    fun prepare(text: String, streaming: Boolean = false): String {
        if (!text.contains('$') && !text.contains("\\(") && !text.contains("\\[") && !text.contains("\\begin{")) return text
        val out = StringBuilder(text.length + 32)
        var i = 0
        var lineStart = true
        while (i < text.length) {
            // Fenced code blocks and inline code are copied untouched.
            if (lineStart && text.startsWith("```", i)) {
                val end = text.indexOf("\n```", i + 3).let { if (it < 0) text.length else (text.indexOf('\n', it + 4).let { e -> if (e < 0) text.length else e }) }
                out.append(text, i, end)
                i = end
                continue
            }
            val c = text[i]
            if (c == '`') {
                val end = text.indexOf('`', i + 1)
                if (end > 0) {
                    out.append(text, i, end + 1)
                    i = end + 1
                    lineStart = false
                    continue
                }
            }
            val found: Pair<Int, String>? = when {
                c == '\\' && text.startsWith("\\[", i) -> closing(text, i + 2, "\\]")?.let { end -> block(text.substring(i + 2, end))?.let { end + 2 to it } }
                c == '\\' && text.startsWith("\\(", i) -> closing(text, i + 2, "\\)")?.let { end -> inline(text.substring(i + 2, end))?.let { end + 2 to it } }
                c == '\\' && lineStart && text.startsWith("\\begin{", i) -> environment(text, i)
                c == '$' && text.startsWith("$$", i) -> closing(text, i + 2, "$$")?.let { end ->
                    val content = text.substring(i + 2, end)
                    val own = lineStart && (end + 2 >= text.length || text[end + 2] == '\n')
                    (if (own) block(content) else inline(content))?.let { end + 2 to it }
                }
                c == '$' -> dollar(text, i)
                else -> null
            }
            if (found != null) {
                out.append(found.second)
                i = found.first
                lineStart = false
                continue
            }
            // Mid-answer, an opened formula without its end is not drawn half-written.
            if (streaming && opensUnfinished(text, i, lineStart)) {
                out.setLength(out.trimEnd().length)
                out.append(PENDING)
                break
            }
            out.append(c)
            lineStart = c == '\n' || (lineStart && (c == ' ' || c == '\t'))
            i++
        }
        return out.toString()
    }

    private fun opensUnfinished(text: String, i: Int, lineStart: Boolean): Boolean = when {
        text.startsWith("\\[", i) -> closing(text, i + 2, "\\]") == null
        text.startsWith("\\(", i) -> closing(text, i + 2, "\\)") == null
        text.startsWith("$$", i) -> closing(text, i + 2, "$$") == null
        lineStart && text.startsWith("\\begin{", i) -> text.indexOf('}', i).let { e ->
            e < 0 || (text.substring(i + 7, e) in ENVIRONMENTS && text.indexOf("\\end{" + text.substring(i + 7, e) + "}", e) < 0)
        }
        // `$x + y` on the last line, no closing `$` yet; prices like `$5` are text anyway.
        text[i] == '$' -> (i == 0 || text[i - 1] != '\\') && text.indexOf('\n', i) < 0 &&
            text.getOrNull(i + 1)?.let { !it.isWhitespace() && !it.isDigit() } != false && text.indexOf('$', i + 1) < 0
        else -> false
    }

    private fun closing(text: String, from: Int, marker: String): Int? {
        var j = from
        while (j < text.length) {
            if (text.startsWith(marker, j)) return j
            if (text[j] == '\\' && marker != "\\]" && marker != "\\)") {
                j += 2
                continue
            }
            j++
        }
        return null
    }

    /** One line; `\\[6pt]` row gaps, which JLatexMath prints as text, become plain `\\`. */
    private fun clean(content: String) = content.trim().replace(Regex("\\s*\\n\\s*"), " ")
        .replace(Regex("\\\\\\\\\\s*\\[[^\\]]*\\]"), "\\\\\\\\")

    private fun block(content: String): String? {
        val tex = clean(content).takeIf { it.isNotEmpty() && balanced(it) } ?: return null
        return "\n$$\n$tex\n$$\n"
    }

    private fun inline(content: String): String? {
        val tex = clean(content).takeIf { it.isNotEmpty() && balanced(it) } ?: return null
        return "$$" + tex + "$$"
    }

    /** `$x$`: no space just inside the dollars and no digit right after, so `$5 и $10` stays text. */
    private fun dollar(text: String, start: Int): Pair<Int, String>? {
        if (start > 0 && text[start - 1] == '\\') return null
        val first = text.getOrNull(start + 1) ?: return null
        if (first.isWhitespace() || first == '$') return null
        var j = start + 1
        while (j < text.length && text[j] != '\n') {
            if (text[j] == '\\') {
                j += 2
                continue
            }
            if (text[j] == '$') {
                if (text[j - 1].isWhitespace()) return null
                if (text.getOrNull(j + 1)?.isDigit() == true) return null
                return inline(text.substring(start + 1, j))?.let { j + 1 to it }
            }
            j++
        }
        return null
    }

    private fun environment(text: String, start: Int): Pair<Int, String>? {
        val nameEnd = text.indexOf('}', start + 7).takeIf { it > 0 } ?: return null
        val name = text.substring(start + 7, nameEnd)
        if (name !in ENVIRONMENTS) return null
        val endTag = "\\end{$name}"
        val close = text.indexOf(endTag, nameEnd).takeIf { it > 0 } ?: return null
        val body = text.substring(nameEnd + 1, close)
        val tex = when (name.trimEnd('*')) {
            "equation" -> body
            "align", "eqnarray", "split" -> "\\begin{aligned}" + body.replace("&=&", "&=") + "\\end{aligned}"
            "gather", "multline" -> "\\begin{gathered}$body\\end{gathered}"
            else -> text.substring(start, close + endTag.length)
        }.replace("\\nonumber", "").replace("\\notag", "").replace(Regex("\\\\label\\{[^}]*\\}"), "")
        return block(tex)?.let { close + endTag.length to it }
    }

    private fun balanced(s: String): Boolean {
        var depth = 0
        var i = 0
        while (i < s.length) {
            when (s[i]) {
                '\\' -> i++
                '{' -> depth++
                '}' -> if (--depth < 0) return false
            }
            i++
        }
        return depth == 0
    }
}
