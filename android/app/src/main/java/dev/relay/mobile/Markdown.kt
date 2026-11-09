package dev.relay.mobile

import android.content.Context
import android.graphics.Typeface
import android.text.method.LinkMovementMethod
import android.util.TypedValue
import android.widget.TextView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import io.noties.markwon.AbstractMarkwonPlugin
import io.noties.markwon.Markwon
import io.noties.markwon.core.MarkwonTheme
import io.noties.markwon.ext.latex.JLatexMathPlugin
import io.noties.markwon.ext.strikethrough.StrikethroughPlugin
import io.noties.markwon.ext.tables.TablePlugin
import io.noties.markwon.inlineparser.MarkwonInlineParserPlugin
import org.scilab.forge.jlatexmath.DefaultTeXFont
import org.scilab.forge.jlatexmath.cyrillic.CyrillicRegistration
import ru.noties.jlatexmath.JLatexMathAndroid

private const val TEXT_SP = 15f

object Formulas {
    @Volatile private var ready = false
    /** JLatexMath fonts load once; Cyrillic lets \text{скорость} render as letters, not boxes. */
    fun init(context: Context) {
        if (ready) return
        synchronized(this) {
            if (ready) return
            JLatexMathAndroid.init(context.applicationContext)
            runCatching { DefaultTeXFont.registerAlphabet(CyrillicRegistration()) }
            ready = true
        }
    }
}

fun markwon(context: Context, p: Palette): Markwon {
    Formulas.init(context)
    val px = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, TEXT_SP, context.resources.displayMetrics)
    return Markwon.builder(context)
        .usePlugin(MarkwonInlineParserPlugin.create())
        .usePlugin(JLatexMathPlugin.create(px * 1.1f) { builder ->
            builder.inlinesEnabled(true)
            builder.blocksEnabled(true)
            builder.theme().textColor(p.text.toArgb())
            builder.errorHandler { latex, _ -> null.also { android.util.Log.w("Relay", "LaTeX: $latex") } }
        })
        .usePlugin(TablePlugin.create(context))
        .usePlugin(StrikethroughPlugin.create())
        .usePlugin(object : AbstractMarkwonPlugin() {
            override fun configureTheme(builder: MarkwonTheme.Builder) {
                builder
                    .linkColor(p.accent.toArgb())
                    .codeTextColor(p.text.toArgb())
                    .codeBackgroundColor(p.surface.toArgb())
                    .codeBlockTextColor(p.text.toArgb())
                    .codeBlockBackgroundColor(p.surface.toArgb())
                    .codeTypeface(Typeface.MONOSPACE)
                    .blockQuoteColor(p.line.toArgb())
                    .listItemColor(p.muted.toArgb())
                    .thematicBreakColor(p.line.toArgb())
                    .headingBreakHeight(0)
            }
        })
        .build()
}

/** Markdown with formulas, rendered by Markwon into a selectable TextView. */
@Composable
fun MarkdownText(text: String, p: Palette, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val engine = remember(p) { markwon(context, p) }
    val prepared = remember(text) { MathText.prepare(text) }
    AndroidView(
        modifier = modifier,
        factory = { ctx ->
            TextView(ctx).apply {
                setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP)
                setLineSpacing(0f, 1.15f)
                movementMethod = LinkMovementMethod.getInstance()
                setTextIsSelectable(true)
            }
        },
        update = { view ->
            view.setTextColor(p.text.toArgb())
            view.highlightColor = p.accent.copy(alpha = 0.3f).toArgb()
            if (view.tag != prepared to p) {
                engine.setMarkdown(view, prepared)
                view.tag = prepared to p
            }
        },
    )
}
