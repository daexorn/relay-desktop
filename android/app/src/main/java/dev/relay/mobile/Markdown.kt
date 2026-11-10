package dev.relay.mobile

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.text.Layout
import android.text.method.LinkMovementMethod
import android.text.style.AlignmentSpan
import android.text.style.ReplacementSpan
import android.util.LruCache
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
import io.noties.markwon.MarkwonPlugin
import io.noties.markwon.MarkwonVisitor
import io.noties.markwon.core.MarkwonTheme
import io.noties.markwon.ext.latex.JLatexMathBlock
import io.noties.markwon.ext.latex.JLatexMathNode
import io.noties.markwon.ext.latex.JLatexMathPlugin
import io.noties.markwon.ext.strikethrough.StrikethroughPlugin
import io.noties.markwon.ext.tables.TablePlugin
import io.noties.markwon.inlineparser.MarkwonInlineParserPlugin
import org.scilab.forge.jlatexmath.DefaultTeXFont
import org.scilab.forge.jlatexmath.cyrillic.CyrillicRegistration
import ru.noties.jlatexmath.JLatexMathAndroid
import ru.noties.jlatexmath.JLatexMathDrawable
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

private const val TEXT_SP = 15f

object Formulas {
    @Volatile private var ready = false
    /** Typeset formulas by size, colour and source: redrawing a growing message reuses them, so nothing blinks. */
    private val cache = LruCache<String, Result<JLatexMathDrawable>>(256)

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

    fun drawable(latex: String, size: Float, color: Int): JLatexMathDrawable? {
        val key = "$size|$color|$latex"
        val done = cache.get(key) ?: runCatching {
            JLatexMathDrawable.builder(latex).textSize(size).color(color).padding(0).fitCanvas(false).build()
                .also { it.setBounds(0, 0, it.intrinsicWidth, it.intrinsicHeight) }
        }.also {
            it.exceptionOrNull()?.let { e -> android.util.Log.w("Relay", "LaTeX: $latex", e) }
            cache.put(key, it)
        }
        return done.getOrNull()
    }
}

/**
 * A typeset formula standing on the text baseline: its TeX depth goes below the line, the rest
 * above, so `x_i` inside a sentence lines up with the words. Block formulas shrink to fit.
 */
class MathSpan(private val math: JLatexMathDrawable, private val block: Boolean, private val room: () -> Int) : ReplacementSpan() {
    private val pad = if (block) (math.intrinsicHeight * 0.12f).roundToInt().coerceIn(4, 24) else 0

    private fun scale(): Float {
        val w = room()
        return if (w > 0 && math.intrinsicWidth > w) w.toFloat() / math.intrinsicWidth else 1f
    }

    override fun getSize(paint: Paint, text: CharSequence?, start: Int, end: Int, fm: Paint.FontMetricsInt?): Int {
        val s = scale()
        val icon = math.icon()
        val depth = (icon.iconDepth * s).roundToInt()
        val height = (math.intrinsicHeight * s).roundToInt()
        if (fm != null) {
            val font = paint.fontMetricsInt
            fm.ascent = min(font.ascent, -(height - depth) - pad)
            fm.descent = max(font.descent, depth + pad)
            fm.top = min(font.top, fm.ascent)
            fm.bottom = max(font.bottom, fm.descent)
            fm.leading = font.leading
        }
        return (math.intrinsicWidth * s).roundToInt() + if (block) 0 else 2
    }

    override fun draw(canvas: Canvas, text: CharSequence?, start: Int, end: Int, x: Float, top: Int, y: Int, bottom: Int, paint: Paint) {
        val s = scale()
        val depth = math.icon().iconDepth * s
        canvas.save()
        canvas.translate(x + if (block) 0f else 1f, y + depth - math.intrinsicHeight * s)
        canvas.scale(s, s)
        math.draw(canvas)
        canvas.restore()
    }
}

/** Keeps JLatexMathPlugin's parsers but draws formulas with [MathSpan], synchronously and cached. */
private class MathSpans(private val size: Float, private val color: Int, private val room: () -> Int) : AbstractMarkwonPlugin() {
    override fun configure(registry: MarkwonPlugin.Registry) {
        registry.require(JLatexMathPlugin::class.java)
    }

    override fun configureVisitor(builder: MarkwonVisitor.Builder) {
        builder.on(JLatexMathNode::class.java) { visitor, node -> put(visitor, node.latex(), false) }
        builder.on(JLatexMathBlock::class.java) { visitor, node ->
            visitor.blockStart(node)
            put(visitor, node.latex(), true)
            visitor.blockEnd(node)
        }
    }

    private fun put(visitor: MarkwonVisitor, latex: String, block: Boolean) {
        val source = latex.trim()
        val start = visitor.length()
        visitor.builder().append(source.replace('\n', ' '))
        val math = Formulas.drawable(source, if (block) size * 1.12f else size, color) ?: return
        visitor.setSpans(start, MathSpan(math, block, room))
        if (block) visitor.setSpans(start, AlignmentSpan.Standard(Layout.Alignment.ALIGN_CENTER))
    }
}

fun markwon(context: Context, p: Palette, room: () -> Int = { 0 }, onFile: ((String) -> Unit)? = null): Markwon {
    Formulas.init(context)
    val px = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, TEXT_SP, context.resources.displayMetrics)
    return Markwon.builder(context)
        .usePlugin(MarkwonInlineParserPlugin.create())
        .usePlugin(JLatexMathPlugin.create(px) { builder ->
            builder.inlinesEnabled(true)
            builder.blocksEnabled(true)
        })
        .usePlugin(MathSpans(px, p.text.toArgb(), room))
        .usePlugin(TablePlugin.create(context))
        .usePlugin(StrikethroughPlugin.create())
        .usePlugin(object : AbstractMarkwonPlugin() {
            // Links to files on the server download to the phone; web links open in the browser.
            override fun configureConfiguration(builder: io.noties.markwon.MarkwonConfiguration.Builder) {
                val web = io.noties.markwon.LinkResolverDef()
                builder.linkResolver { view, link ->
                    if (onFile != null && Downloads.isServerFile(link)) onFile(Downloads.serverPath(link))
                    else runCatching { web.resolve(view, link) }
                }
            }

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

/**
 * Markdown with formulas, rendered by Markwon into a selectable TextView. While Claude is still
 * writing, a formula that is not closed yet stays hidden until its closing marker arrives.
 */
@Composable
fun MarkdownText(text: String, p: Palette, modifier: Modifier = Modifier, streaming: Boolean = false, onFile: ((String) -> Unit)? = null) {
    val context = LocalContext.current
    val holder = remember { arrayOfNulls<TextView>(1) }
    val engine = remember(p) {
        val fallback = context.resources.displayMetrics.widthPixels - (40 * context.resources.displayMetrics.density).roundToInt()
        markwon(context, p, { holder[0]?.let { it.width - it.paddingLeft - it.paddingRight }?.takeIf { it > 0 } ?: fallback }, onFile)
    }
    val prepared = remember(text, streaming) { MathText.prepare(text, streaming) }
    AndroidView(
        modifier = modifier,
        factory = { ctx ->
            TextView(ctx).apply {
                setTextSize(TypedValue.COMPLEX_UNIT_SP, TEXT_SP)
                setLineSpacing(0f, 1.15f)
                movementMethod = LinkMovementMethod.getInstance()
                setTextIsSelectable(true)
                holder[0] = this
            }
        },
        update = { view ->
            holder[0] = view
            view.setTextColor(p.text.toArgb())
            view.highlightColor = p.accent.copy(alpha = 0.3f).toArgb()
            if (view.tag != prepared to p) {
                engine.setMarkdown(view, prepared)
                view.tag = prepared to p
            }
        },
    )
}
