package dev.relay.testbot;

import com.google.gson.GsonBuilder;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.atomic.AtomicReference;
import net.fabricmc.api.ClientModInitializer;
import net.minecraft.client.MinecraftClient;
import net.minecraft.client.gui.Element;
import net.minecraft.client.gui.ParentElement;
import net.minecraft.client.gui.screen.Screen;
import net.minecraft.client.gui.screen.TitleScreen;
import net.minecraft.client.gui.screen.multiplayer.ConnectScreen;
import net.minecraft.client.gui.screen.world.CreateWorldScreen;
import net.minecraft.client.gui.widget.ClickableWidget;
import net.minecraft.client.gui.widget.PressableWidget;
import net.minecraft.client.network.ServerAddress;
import net.minecraft.client.network.ServerInfo;
import net.minecraft.client.option.KeyBinding;
import net.minecraft.client.util.InputUtil;
import net.minecraft.client.util.ScreenshotRecorder;
import net.minecraft.text.Text;
import net.minecraft.text.TranslatableTextContent;

/**
 * Runs a line-based scenario from {@code -Drelay.test.scenario} one step per client tick and
 * writes {@code report.json}, {@code steps.log} and screenshots into {@code -Drelay.test.out}.
 * Without the property the mod does nothing, so a stray copy in a normal profile is harmless.
 */
public final class TestBot implements ClientModInitializer {
    private static final long DEFAULT_TIMEOUT_MS = 60_000;

    private static Path out;
    private static List<String> lines = List.of();
    private static int index;
    private static long stepStart;
    private static long started;
    private static long deadline;
    private static boolean prepared;
    private static boolean finished;
    private static boolean failing;
    private static InputUtil.Key releaseKey;
    private static final AtomicReference<String> shot = new AtomicReference<>();
    private static final List<Map<String, Object>> results = new ArrayList<>();
    private static final StringBuilder log = new StringBuilder();

    @Override
    public void onInitializeClient() {
        String scenario = System.getProperty("relay.test.scenario");
        if (scenario == null) {
            return;
        }
        try {
            out = Path.of(System.getProperty("relay.test.out", "relay-test-out"));
            Files.createDirectories(out);
            lines = Files.readAllLines(Path.of(scenario), StandardCharsets.UTF_8).stream()
                .map(String::strip)
                .filter(l -> !l.isEmpty() && !l.startsWith("#"))
                .toList();
        } catch (IOException e) {
            throw new IllegalStateException("Relay test bot: cannot read scenario " + scenario, e);
        }
        long total = Long.getLong("relay.test.timeout", 600) * 1000;
        started = System.currentTimeMillis();
        deadline = started + total;
        note("scenario " + scenario + " · " + lines.size() + " steps");
    }

    public static boolean hidden() {
        return System.getProperty("relay.test.scenario") != null
            && !"false".equals(System.getProperty("relay.test.hidden"));
    }

    public static void tick(MinecraftClient client) {
        if (out == null || finished) {
            return;
        }
        if (releaseKey != null) {
            KeyBinding.setKeyPressed(releaseKey, false);
            releaseKey = null;
        }
        if (!prepared) {
            prepared = true;
            // A hidden window never has focus; without this the game pauses itself.
            client.options.pauseOnLostFocus = false;
            client.options.onboardAccessibility = false;
            client.options.skipMultiplayerWarning = true;
            client.options.write();
            stepStart = System.currentTimeMillis();
        }
        if (System.currentTimeMillis() > deadline && !failing) {
            fail(client, "общий таймаут сценария");
            return;
        }
        if (index >= lines.size()) {
            finish(client, true);
            return;
        }
        // The loading overlay draws over whatever screen already exists underneath it.
        if (client.getOverlay() != null) {
            stepStart = System.currentTimeMillis();
            return;
        }
        String line = lines.get(index);
        String[] parts = line.split("\\s+", 2);
        String op = parts[0].toLowerCase(Locale.ROOT);
        String arg = parts.length > 1 ? unquote(parts[1]) : "";
        long elapsed = System.currentTimeMillis() - stepStart;
        try {
            Boolean done = step(client, op, arg, elapsed);
            if (done == null) {
                return;
            }
            if (done) {
                pass(line, elapsed);
            } else if (elapsed > timeoutOf(op, arg)) {
                fail(client, "шаг не дождался результата: " + line);
            }
        } catch (RuntimeException e) {
            fail(client, line + " → " + e);
        }
    }

    /** true = step finished, false = keep waiting, null = scenario already ended. */
    private static Boolean step(MinecraftClient client, String op, String arg, long elapsed) {
        switch (op) {
            case "wait" -> {
                return elapsed >= (long) (Double.parseDouble(arg) * 1000);
            }
            case "wait_screen" -> {
                String want = arg.split("\\s+")[0];
                Screen s = client.currentScreen;
                if (want.equalsIgnoreCase("none")) {
                    return s == null;
                }
                return s != null && (contains(s.getClass().getSimpleName(), want) || contains(s.getTitle().getString(), want));
            }
            case "wait_world" -> {
                return client.world != null && client.player != null && client.currentScreen == null;
            }
            case "test_world" -> {
                if (elapsed == 0 || !(client.currentScreen instanceof CreateWorldScreen)) {
                    if (!(client.currentScreen instanceof CreateWorldScreen)) {
                        CreateWorldScreen.showTestWorld(client, new TitleScreen());
                    }
                    return false;
                }
                return click(client.currentScreen, "key:selectWorld.create");
            }
            case "click" -> {
                return client.currentScreen != null && click(client.currentScreen, arg);
            }
            case "screen" -> {
                note(describe(client.currentScreen));
                return true;
            }
            case "screenshot" -> {
                return screenshot(client, arg.isEmpty() ? "shot-" + index : arg, elapsed);
            }
            case "join" -> {
                ServerAddress address = ServerAddress.parse(arg);
                ServerInfo info = new ServerInfo("Relay test", arg, ServerInfo.ServerType.OTHER);
                ConnectScreen.connect(new TitleScreen(), client, address, info, false, null);
                return true;
            }
            case "command" -> {
                client.player.networkHandler.sendChatCommand(arg.startsWith("/") ? arg.substring(1) : arg);
                return true;
            }
            case "chat" -> {
                client.player.networkHandler.sendChatMessage(arg);
                return true;
            }
            case "key" -> {
                press(client, arg);
                return true;
            }
            case "log" -> {
                note(arg);
                return true;
            }
            case "quit" -> {
                finish(client, true);
                return null;
            }
            default -> throw new IllegalArgumentException("неизвестный шаг " + op);
        }
    }

    private static long timeoutOf(String op, String arg) {
        String[] words = arg.split("\\s+");
        if (op.startsWith("wait_") && words.length > 1) {
            return (long) (Double.parseDouble(words[words.length - 1]) * 1000);
        }
        return op.equals("test_world") || op.equals("wait_world") ? 180_000 : DEFAULT_TIMEOUT_MS;
    }

    private static boolean screenshot(MinecraftClient client, String name, long elapsed) {
        if (elapsed == 0 || shot.get() == null) {
            if (shot.compareAndSet(null, "pending:" + name)) {
                Path file = out.resolve(name.replaceAll("[^\\w.-]", "_") + ".png");
                ScreenshotRecorder.takeScreenshot(client.getFramebuffer(), image -> {
                    try (image) {
                        image.writeTo(file);
                        shot.set("done:" + file.getFileName());
                    } catch (IOException e) {
                        shot.set("error:" + e);
                    }
                });
            }
            return false;
        }
        String state = shot.get();
        if (state.startsWith("done:")) {
            note("screenshot " + state.substring(5));
            shot.set(null);
            return true;
        }
        if (state.startsWith("error:")) {
            shot.set(null);
            throw new IllegalStateException(state);
        }
        return false;
    }

    private static boolean click(Screen screen, String target) {
        for (ClickableWidget widget : widgets(screen, new ArrayList<>())) {
            if (widget.visible && widget.active && matches(widget.getMessage(), target)) {
                if (widget instanceof PressableWidget pressable) {
                    pressable.onPress();
                } else {
                    widget.mouseClicked(widget.getX() + widget.getWidth() / 2.0, widget.getY() + widget.getHeight() / 2.0, 0);
                }
                note("click «" + widget.getMessage().getString() + "»");
                return true;
            }
        }
        return false;
    }

    private static List<ClickableWidget> widgets(ParentElement parent, List<ClickableWidget> found) {
        for (Element child : parent.children()) {
            if (child instanceof ClickableWidget widget) {
                found.add(widget);
            }
            if (child instanceof ParentElement nested) {
                widgets(nested, found);
            }
        }
        return found;
    }

    private static boolean matches(Text text, String target) {
        if (target.startsWith("key:")) {
            return text.getContent() instanceof TranslatableTextContent t && t.getKey().equals(target.substring(4));
        }
        return contains(text.getString(), target);
    }

    private static void press(MinecraftClient client, String name) {
        KeyBinding binding = null;
        for (KeyBinding k : client.options.allKeys) {
            if (k.getTranslationKey().equalsIgnoreCase(name)) {
                binding = k;
            }
        }
        InputUtil.Key key = binding != null
            ? InputUtil.fromTranslationKey(binding.getBoundKeyTranslationKey())
            : InputUtil.fromTranslationKey(name.contains(".") ? name : "key.keyboard." + name.toLowerCase(Locale.ROOT));
        if (client.currentScreen != null) {
            client.currentScreen.keyPressed(key.getCode(), 0, 0);
        } else {
            KeyBinding.onKeyPressed(key);
            KeyBinding.setKeyPressed(key, true);
            releaseKey = key;
        }
        note("key " + key.getTranslationKey());
    }

    private static String describe(Screen screen) {
        if (screen == null) {
            return "screen: нет (игровой мир)";
        }
        StringBuilder b = new StringBuilder("screen: " + screen.getClass().getName() + " «" + screen.getTitle().getString() + "»");
        for (ClickableWidget w : widgets(screen, new ArrayList<>())) {
            b.append("\n  - ").append(w.getClass().getSimpleName()).append(" «").append(w.getMessage().getString()).append("»");
            if (w.getMessage().getContent() instanceof TranslatableTextContent t) {
                b.append(" key:").append(t.getKey());
            }
            if (!w.active) {
                b.append(" (неактивна)");
            }
        }
        return b.toString();
    }

    private static void pass(String line, long elapsed) {
        Map<String, Object> r = new LinkedHashMap<>();
        r.put("step", line);
        r.put("ok", true);
        r.put("ms", elapsed);
        results.add(r);
        note("✓ " + line + " (" + elapsed + " мс)");
        index++;
        stepStart = System.currentTimeMillis();
    }

    private static void fail(MinecraftClient client, String reason) {
        if (failing) {
            return;
        }
        failing = true;
        Map<String, Object> r = new LinkedHashMap<>();
        r.put("step", index < lines.size() ? lines.get(index) : "");
        r.put("ok", false);
        r.put("error", reason);
        results.add(r);
        note("✗ " + reason);
        note(describe(client.currentScreen));
        // Replace the rest of the scenario with a failure shot and quit.
        lines = new ArrayList<>(lines.subList(0, index));
        lines.add("screenshot failure");
        lines.add("quit");
        stepStart = System.currentTimeMillis();
        deadline = System.currentTimeMillis() + 30_000;
    }

    private static void finish(MinecraftClient client, boolean quit) {
        finished = true;
        Map<String, Object> report = new LinkedHashMap<>();
        report.put("ok", !failing);
        report.put("seconds", (System.currentTimeMillis() - started) / 1000.0);
        report.put("steps", results);
        report.put("gpu", com.mojang.blaze3d.systems.RenderSystem.getDevice().getRenderer());
        try {
            Files.writeString(out.resolve("report.json"), new GsonBuilder().setPrettyPrinting().create().toJson(report));
            Files.writeString(out.resolve("steps.log"), log);
        } catch (IOException ignored) {
            // The game log still has every note.
        }
        if (quit) {
            client.scheduleStop();
        }
    }

    private static void note(String text) {
        String line = String.format("[%6.1f] %s", (System.currentTimeMillis() - started) / 1000.0, text);
        log.append(line).append('\n');
        System.out.println("[relay-test] " + line);
    }

    private static boolean contains(String haystack, String needle) {
        return haystack.toLowerCase(Locale.ROOT).contains(needle.toLowerCase(Locale.ROOT));
    }

    private static String unquote(String s) {
        return s.length() > 1 && s.startsWith("\"") && s.endsWith("\"") ? s.substring(1, s.length() - 1) : s;
    }
}
