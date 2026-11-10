package dev.relay.mobile

import android.graphics.Bitmap
import android.graphics.Canvas
import android.view.View
import android.widget.TextView
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File

/** Draws a message with inline and display formulas exactly as the chat does and saves it. */
@RunWith(AndroidJUnit4::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34], qualifiers = "w400dp-h860dp-xxhdpi")
class FormulaRenderTest {
    private val sample = """
Здесь §\omega_i = \sqrt{g\,k_i}§ (дисперсия на глубокой воде), а §k_i = \frac{2\pi}{\lambda_i}§.

$$
\vec P(x,z,t) = \begin{pmatrix} x + \sum_i \frac{Q_i}{k_i} D_{i,x}\cos\theta_i \\[6pt] \sum_i A_i \sin\theta_i \\[6pt] z + \sum_i \frac{Q_i}{k_i} D_{i,z}\cos\theta_i \end{pmatrix}
$$

И классика напоследок: §e^{i\pi} + 1 = 0§. Индексы §x_i§, §y^2§ и текст в одной строке.
""".trimIndent().replace('§', '$')

    @Test
    fun formulasSitOnTheBaseline() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val p = palette("graphite")
        val view = TextView(context).apply {
            textSize = 15f
            setLineSpacing(0f, 1.15f)
            setTextColor(android.graphics.Color.WHITE)
            setBackgroundColor(android.graphics.Color.rgb(0x19, 0x19, 0x1b))
            setPadding(40, 30, 40, 30)
        }
        val width = 1200
        markwon(context, p) { width - 80 }.setMarkdown(view, MathText.prepare(sample))
        view.measure(View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(0, View.MeasureSpec.UNSPECIFIED))
        view.layout(0, 0, width, view.measuredHeight)
        val bitmap = Bitmap.createBitmap(width, view.measuredHeight, Bitmap.Config.ARGB_8888)
        view.draw(Canvas(bitmap))
        File("build/screens").apply { mkdirs() }.resolve("08-formulas.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
        val spans = (view.text as android.text.Spanned).getSpans(0, view.text.length, MathSpan::class.java)
        assertTrue("formulas drawn: ${spans.size}", spans.size == 6)
        assertTrue("row gap leaked", !view.text.contains("6pt"))
    }
}
