package dev.relay.mobile

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MathTextTest {
    private fun p(s: String) = MathText.prepare(s)

    @Test fun inlineDollarsBecomeMarkwonInline() =
        assertEquals("пункт с $\$x^2$$, и $\$y_1$$.", p("пункт с \$x^2\$, и \$y_1\$."))

    @Test fun pricesStayText() = assertEquals("Цена \$5 и \$10, формула $\$a+b$$.", p("Цена \$5 и \$10, формула \$a+b\$."))

    @Test fun parenAndBracketForms() {
        assertEquals("В тексте $\$\\alpha$$ и $\$\\beta$$.", p("В тексте \\(\\alpha\\) и \\(\\beta\\)."))
        assertEquals("Решение:\n\n$$\n\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}\n$$\n",
            p("Решение:\n\\[\n\\begin{aligned}\na &= b \\\\\nc &= d\n\\end{aligned}\n\\]"))
    }

    @Test fun displayOnItsOwnLinesIsABlock() {
        assertEquals("Формула:\n\n$$\n\\int_0^1 x\\,dx\n$$\n\nдальше", p("Формула:\n$$\n\\int_0^1 x\\,dx\n$$\nдальше"))
        assertEquals("Ответ: $\$x = 1$$.", p("Ответ: $\$x = 1$$."))
    }

    @Test fun bareEnvironmentsBecomeBlocks() {
        assertEquals("\n$$\nE = mc^2\n$$\n", p("\\begin{equation}\nE = mc^2 \\label{e}\n\\end{equation}"))
        val aligned = p("Итак:\n\\begin{align}\nx &= 1 \\nonumber \\\\\ny &= 2\n\\end{align}")
        assertTrue(aligned, aligned.contains("$$\n\\begin{aligned} x &= 1  \\\\ y &= 2 \\end{aligned}\n$$"))
        assertEquals("слово \\begin{equation}x\\end{equation}", p("слово \\begin{equation}x\\end{equation}"))
        assertEquals("\\begin{itemize}\n\\item x\n\\end{itemize}", p("\\begin{itemize}\n\\item x\n\\end{itemize}"))
    }

    @Test fun codeIsUntouched() {
        val code = "```latex\n\\frac{a}{b} \$x\$\n```"
        assertEquals(code, p(code))
        assertEquals("`echo \$HOME \$PATH`", p("`echo \$HOME \$PATH`"))
    }

    @Test fun brokenBracesStayText() = assertFalse(p("\\[ \\frac{1}{ \\]").contains("$$"))

    @Test fun unfinishedFormulasWaitWhileStreaming() {
        val s = { t: String -> MathText.prepare(t, streaming = true) }
        assertEquals("Сумма:" + MathText.PENDING, s("Сумма:\n$$\n\\sum_i A_i"))
        assertEquals("Пусть" + MathText.PENDING, s("Пусть \\(x_"))
        assertEquals("Пусть" + MathText.PENDING, s("Пусть \$\\omega_i = \\sqrt{g"))
        assertEquals("Цена \$5", s("Цена \$5"))
        assertEquals("Пусть $\$x$$ и", s("Пусть \$x\$ и"))
        assertTrue(s("\\begin{pmatrix} a").endsWith(MathText.PENDING))
        // Without streaming the same text stays as it is.
        assertEquals("Пусть \\(x_", p("Пусть \\(x_"))
    }

    @Test fun rowGapsAreDropped() =
        assertEquals("\n$$\n\\begin{pmatrix} a \\\\ b \\end{pmatrix}\n$$\n", p("\\begin{pmatrix}\na \\\\[6pt]\nb\n\\end{pmatrix}"))
}
