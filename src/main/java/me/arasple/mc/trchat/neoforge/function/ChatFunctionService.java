package me.arasple.mc.trchat.neoforge.function;

import me.arasple.mc.trchat.neoforge.channel.ConditionEvaluator;
import me.arasple.mc.trchat.neoforge.chat.LegacyText;
import me.arasple.mc.trchat.neoforge.permission.TrChatPermissions;
import net.minecraft.ChatFormatting;
import net.minecraft.network.chat.ClickEvent;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.HoverEvent;
import net.minecraft.network.chat.MutableComponent;
import net.minecraft.network.chat.Style;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.sounds.SoundEvents;
import net.minecraft.sounds.SoundSource;
import net.minecraft.world.SimpleContainer;
import net.minecraft.world.SimpleMenuProvider;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.inventory.ChestMenu;
import net.minecraft.world.item.ItemStack;
import net.neoforged.fml.loading.FMLPaths;
import org.yaml.snakeyaml.LoaderOptions;
import org.yaml.snakeyaml.Yaml;
import org.yaml.snakeyaml.constructor.SafeConstructor;

import java.io.IOException;
import java.io.InputStream;
import java.io.Reader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.regex.PatternSyntaxException;

public final class ChatFunctionService {

    private static final System.Logger LOGGER = System.getLogger(ChatFunctionService.class.getName());
    private static final Duration SNAPSHOT_TTL = Duration.ofMinutes(5);
    private static final Pattern PROPERTY = Pattern.compile("\\{([^}:]+):\\s*([^}]*)}");

    private final MinecraftServer server;
    private final Path file;
    private final Map<String, Long> cooldowns = new HashMap<>();
    private final Map<String, Snapshot> snapshots = new LinkedHashMap<>();
    private volatile Configuration configuration = Configuration.empty();

    public ChatFunctionService(MinecraftServer server) {
        this(server, FMLPaths.CONFIGDIR.get().resolve("trchat-neoforge").resolve("function.yml"));
    }

    ChatFunctionService(MinecraftServer server, Path file) {
        this.server = server;
        this.file = file;
    }

    public synchronized boolean reload() {
        try {
            Files.createDirectories(file.getParent());
            if (!Files.exists(file)) {
                try (InputStream input = ChatFunctionService.class.getResourceAsStream("/defaults/function.yml")) {
                    if (input == null) throw new IOException("Missing bundled function.yml");
                    Files.copy(input, file);
                }
            }
            LoaderOptions loaderOptions = new LoaderOptions();
            loaderOptions.setAllowDuplicateKeys(false);
            Yaml yaml = new Yaml(new SafeConstructor(loaderOptions));
            try (Reader reader = Files.newBufferedReader(file, StandardCharsets.UTF_8)) {
                Object loaded = yaml.load(reader);
                if (!(loaded instanceof Map<?, ?> root)) {
                    throw new IOException("function.yml root must be a mapping");
                }
                configuration = Configuration.from(root);
            }
            cooldowns.clear();
            LOGGER.log(System.Logger.Level.INFO, "Loaded chat functions from {0}", file);
            return true;
        } catch (IOException | RuntimeException exception) {
            LOGGER.log(System.Logger.Level.ERROR, "Unable to load " + file, exception);
            return false;
        }
    }

    public ProcessedMessage process(ServerPlayer sender, String rawMessage, List<String> disabledFunctions) {
        String message = LegacyText.stripLegacyCodes(rawMessage);
        Set<String> disabled = disabledFunctions.stream()
            .map(ChatFunctionService::normalize)
            .collect(java.util.stream.Collectors.toSet());

        List<Token> tokens = collectTokens(sender, message, disabled);
        if (tokens.isEmpty()) {
            return new ProcessedMessage(Component.literal(message), List.of());
        }
        tokens.sort(Comparator.comparingInt(Token::start).thenComparing(Comparator.comparingInt(Token::priority).reversed()));
        List<Token> accepted = new ArrayList<>();
        int cursor = -1;
        for (Token token : tokens) {
            if (token.start() >= cursor) {
                accepted.add(token);
                cursor = token.end();
            }
        }

        MutableComponent output = Component.empty();
        List<String> mentioned = new ArrayList<>();
        cursor = 0;
        for (Token token : accepted) {
            if (token.start() < cursor) continue;
            output.append(Component.literal(message.substring(cursor, token.start())));
            if (!canUse(sender, token)) {
                output.append(Component.literal(message.substring(token.start(), token.end())));
            } else {
                Component replacement = renderToken(sender, token, mentioned);
                output.append(replacement == null
                    ? Component.literal(message.substring(token.start(), token.end()))
                    : replacement);
            }
            cursor = token.end();
        }
        output.append(Component.literal(message.substring(cursor)));
        return new ProcessedMessage(output, List.copyOf(mentioned));
    }

    public boolean openSnapshot(ServerPlayer viewer, String id) {
        expireSnapshots();
        Snapshot snapshot = snapshots.get(id);
        if (snapshot == null) {
            viewer.sendSystemMessage(Component.literal("This inventory snapshot has expired."));
            return false;
        }
        SimpleContainer container = new SimpleContainer(snapshot.size());
        for (int index = 0; index < snapshot.items().size() && index < snapshot.size(); index++) {
            container.setItem(index, snapshot.items().get(index).copy());
        }
        viewer.openMenu(new SimpleMenuProvider(
            (containerId, inventory, ignored) -> snapshot.size() == 54
                ? ChestMenu.sixRows(containerId, inventory, container)
                : ChestMenu.threeRows(containerId, inventory, container),
            Component.literal(snapshot.title())
        ));
        return true;
    }

    public boolean checkCommand(ServerPlayer player, String commandLine) {
        Configuration config = configuration;
        if (!config.commandControllerEnabled()) {
            return true;
        }
        String input = commandLine.startsWith("/") ? commandLine.substring(1) : commandLine;
        String label = input.split("\\s+", 2)[0];
        for (CommandRule rule : config.commandRules()) {
            boolean matches = rule.exact()
                ? rule.pattern().matcher(input).matches()
                : rule.pattern().matcher(label).matches();
            if (!matches) continue;
            if (!rule.condition().isBlank() && !ConditionEvaluator.test(rule.condition(), player)) {
                player.sendSystemMessage(Component.literal("You cannot use this command."));
                return false;
            }
            if (rule.cooldownMillis() > 0 && !cooldown(player, "command:" + rule.source(), rule.cooldownMillis())) {
                player.sendSystemMessage(Component.literal("This command is on cooldown."));
                return false;
            }
            return true;
        }
        return true;
    }

    private List<Token> collectTokens(ServerPlayer sender, String message, Set<String> disabled) {
        Configuration config = configuration;
        List<Token> result = new ArrayList<>();

        if (config.mentionAll().enabled() && !disabled.contains("mentionall")) {
            collectKeys(result, message, config.mentionAll().keys(), Kind.MENTION_ALL, 600, config.mentionAll());
        }
        if (config.inventory().enabled() && !disabled.contains("inventoryshow")) {
            collectKeys(result, message, config.inventory().keys(), Kind.INVENTORY, 550, config.inventory());
        }
        if (config.enderChest().enabled() && !disabled.contains("enderchestshow")) {
            collectKeys(result, message, config.enderChest().keys(), Kind.ENDER_CHEST, 540, config.enderChest());
        }
        if (config.item().enabled() && !disabled.contains("itemshow")) {
            for (String key : config.item().keys()) {
                Pattern pattern = Pattern.compile(Pattern.quote(key) + "-?([1-9])?", Pattern.CASE_INSENSITIVE);
                Matcher matcher = pattern.matcher(message);
                while (matcher.find()) {
                    String slot = matcher.group(1);
                    result.add(new Token(matcher.start(), matcher.end(), 530, Kind.ITEM, slot == null ? "" : slot, config.item(), null));
                }
            }
        }
        if (config.mention().enabled() && !disabled.contains("mention")) {
            List<String> names = server.getPlayerList().getPlayers().stream()
                .map(player -> player.getGameProfile().getName())
                .filter(name -> config.mention().selfMention() || !name.equalsIgnoreCase(sender.getGameProfile().getName()))
                .sorted(Comparator.comparingInt(String::length).reversed())
                .toList();
            if (!names.isEmpty()) {
                String alternatives = String.join("|", names.stream().map(Pattern::quote).toList());
                String configured = config.mention().pattern().replace("(names)", "(" + alternatives + ")");
                try {
                    Matcher matcher = Pattern.compile(configured, Pattern.CASE_INSENSITIVE).matcher(message);
                    while (matcher.find()) {
                        String name = matcher.groupCount() >= 1 ? matcher.group(1) : matcher.group();
                        result.add(new Token(matcher.start(), matcher.end(), 500, Kind.MENTION, name, config.mention(), null));
                    }
                } catch (PatternSyntaxException exception) {
                    LOGGER.log(System.Logger.Level.WARNING, "Invalid Mention.Pattern: " + configured);
                }
            }
        }
        if (!disabled.contains("custom")) {
            for (CustomFunction custom : config.customFunctions()) {
                if (!ConditionEvaluator.test(custom.condition(), sender)) continue;
                Matcher matcher = custom.pattern().matcher(message);
                while (matcher.find()) {
                    String value = matcher.group();
                    if (custom.textFilter() != null) {
                        Matcher filtered = custom.textFilter().matcher(value);
                        value = filtered.find() ? filtered.group() : value;
                    }
                    result.add(new Token(
                        matcher.start(), matcher.end(), custom.priority(), Kind.CUSTOM,
                        value, custom.settings(), custom
                    ));
                }
            }
        }
        return result;
    }

    private static void collectKeys(
        List<Token> output,
        String message,
        List<String> keys,
        Kind kind,
        int priority,
        FunctionSettings settings
    ) {
        for (String key : keys) {
            Matcher matcher = Pattern.compile(Pattern.quote(key), Pattern.CASE_INSENSITIVE).matcher(message);
            while (matcher.find()) {
                output.add(new Token(matcher.start(), matcher.end(), priority, kind, key, settings, null));
            }
        }
    }

    private boolean canUse(ServerPlayer sender, Token token) {
        FunctionSettings settings = token.settings();
        if (!settings.permission().equalsIgnoreCase("none")
            && !TrChatPermissions.check(sender, settings.permission())) {
            return false;
        }
        return settings.cooldownMillis() <= 0
            || cooldown(sender, token.kind().name(), settings.cooldownMillis());
    }

    private boolean cooldown(ServerPlayer player, String function, long cooldownMillis) {
        if (player.hasPermissions(2)) {
            return true;
        }
        String key = player.getUUID() + ":" + function;
        long now = System.currentTimeMillis();
        long until = cooldowns.getOrDefault(key, 0L);
        if (until > now) {
            return false;
        }
        cooldowns.put(key, now + cooldownMillis);
        return true;
    }

    private Component renderToken(ServerPlayer sender, Token token, List<String> mentioned) {
        return switch (token.kind()) {
            case MENTION -> mention(sender, token.argument(), mentioned);
            case MENTION_ALL -> mentionAll(sender, mentioned);
            case ITEM -> item(sender, token.argument());
            case INVENTORY -> inventory(sender, false);
            case ENDER_CHEST -> inventory(sender, true);
            case CUSTOM -> custom(token.custom(), token.argument());
        };
    }

    private Component mention(ServerPlayer sender, String requested, List<String> mentioned) {
        ServerPlayer target = server.getPlayerList().getPlayerByName(requested);
        if (target == null) return Component.literal(requested);
        String name = target.getGameProfile().getName();
        mentioned.add(name);
        if (configuration.mention().notifyPlayer()) {
            target.playNotifySound(SoundEvents.NOTE_BLOCK_PLING.value(), SoundSource.PLAYERS, 1.0F, 1.2F);
            target.sendSystemMessage(Component.literal(sender.getGameProfile().getName() + " mentioned you."));
        }
        return Component.literal("@" + name)
            .withStyle(ChatFormatting.AQUA)
            .withStyle(style -> style.withHoverEvent(new HoverEvent(
                HoverEvent.Action.SHOW_TEXT,
                Component.literal(sender.getGameProfile().getName() + " mentioned " + name)
            )));
    }

    private Component mentionAll(ServerPlayer sender, List<String> mentioned) {
        if (configuration.mentionAll().notifyPlayer()) {
            for (ServerPlayer target : server.getPlayerList().getPlayers()) {
                if (!target.getUUID().equals(sender.getUUID())) {
                    mentioned.add(target.getGameProfile().getName());
                    target.playNotifySound(SoundEvents.NOTE_BLOCK_PLING.value(), SoundSource.PLAYERS, 1.0F, 1.0F);
                }
            }
        }
        return Component.literal("@所有人")
            .withStyle(ChatFormatting.GOLD, ChatFormatting.BOLD)
            .withStyle(style -> style.withHoverEvent(new HoverEvent(
                HoverEvent.Action.SHOW_TEXT,
                Component.literal(sender.getGameProfile().getName() + " mentioned everyone")
            )));
    }

    private Component item(ServerPlayer sender, String slotArgument) {
        int slot = slotArgument.isBlank() ? sender.getInventory().selected : Integer.parseInt(slotArgument) - 1;
        ItemStack stack = sender.getInventory().getItem(slot);
        if (stack.isEmpty()) {
            return Component.literal("[空气]").withStyle(ChatFormatting.GRAY);
        }
        MutableComponent component = Component.literal("[")
            .append(stack.getHoverName())
            .append(Component.literal(" x" + stack.getCount() + "]"));
        return component.withStyle(style -> style
            .withColor(ChatFormatting.AQUA)
            .withHoverEvent(new HoverEvent(HoverEvent.Action.SHOW_ITEM, new HoverEvent.ItemStackInfo(stack))));
    }

    private Component inventory(ServerPlayer sender, boolean enderChest) {
        String snapshotId = createSnapshot(sender, enderChest);
        String label = enderChest ? "[末影箱: " : "[背包: ";
        String hover = enderChest ? "点击查看末影箱快照" : "点击查看背包快照";
        return Component.literal(label + sender.getGameProfile().getName() + "]")
            .withStyle(ChatFormatting.AQUA)
            .withStyle(style -> style
                .withHoverEvent(new HoverEvent(HoverEvent.Action.SHOW_TEXT, Component.literal(hover)))
                .withClickEvent(new ClickEvent(ClickEvent.Action.RUN_COMMAND, "/trchat view " + snapshotId)));
    }

    private String createSnapshot(ServerPlayer player, boolean enderChest) {
        expireSnapshots();
        String id = UUID.randomUUID().toString().replace("-", "").substring(0, 12);
        List<ItemStack> items = new ArrayList<>();
        int size;
        String title;
        if (enderChest) {
            size = 27;
            title = player.getGameProfile().getName() + " - Ender Chest";
            for (int index = 0; index < 27; index++) {
                items.add(player.getEnderChestInventory().getItem(index).copy());
            }
        } else {
            size = 54;
            title = player.getGameProfile().getName() + " - Inventory";
            for (int index = 0; index < 36; index++) {
                items.add(player.getInventory().getItem(index).copy());
            }
            items.add(player.getOffhandItem().copy());
            items.add(player.getItemBySlot(EquipmentSlot.HEAD).copy());
            items.add(player.getItemBySlot(EquipmentSlot.CHEST).copy());
            items.add(player.getItemBySlot(EquipmentSlot.LEGS).copy());
            items.add(player.getItemBySlot(EquipmentSlot.FEET).copy());
        }
        snapshots.put(id, new Snapshot(System.nanoTime(), size, title, List.copyOf(items)));
        while (snapshots.size() > 100) {
            snapshots.remove(snapshots.keySet().iterator().next());
        }
        return id;
    }

    private Component custom(CustomFunction function, String value) {
        if (function == null) return Component.literal(value);
        Display display = function.display();
        MutableComponent component = LegacyText.parse(replace(display.text(), value)).copy();
        Style style = component.getStyle();
        if (!display.hover().isBlank()) {
            style = style.withHoverEvent(new HoverEvent(
                HoverEvent.Action.SHOW_TEXT,
                LegacyText.parse(replace(display.hover(), value))
            ));
        }
        if (!display.url().isBlank()) {
            style = style.withClickEvent(new ClickEvent(ClickEvent.Action.OPEN_URL, replace(display.url(), value)));
        } else if (!display.command().isBlank()) {
            style = style.withClickEvent(new ClickEvent(ClickEvent.Action.RUN_COMMAND, replace(display.command(), value)));
        } else if (!display.suggest().isBlank()) {
            style = style.withClickEvent(new ClickEvent(ClickEvent.Action.SUGGEST_COMMAND, replace(display.suggest(), value)));
        } else if (!display.copy().isBlank()) {
            style = style.withClickEvent(new ClickEvent(ClickEvent.Action.COPY_TO_CLIPBOARD, replace(display.copy(), value)));
        }
        return component.setStyle(style);
    }

    private void expireSnapshots() {
        long cutoff = System.nanoTime() - SNAPSHOT_TTL.toNanos();
        snapshots.entrySet().removeIf(entry -> entry.getValue().createdAtNanos() < cutoff);
    }

    private static String replace(String template, String value) {
        return template.replace("{0}", value);
    }

    private static String normalize(String value) {
        return value.toLowerCase(Locale.ROOT).replace("-", "").replace("_", "").replace(" ", "");
    }

    private enum Kind {
        MENTION,
        MENTION_ALL,
        ITEM,
        INVENTORY,
        ENDER_CHEST,
        CUSTOM
    }

    private record Token(
        int start,
        int end,
        int priority,
        Kind kind,
        String argument,
        FunctionSettings settings,
        CustomFunction custom
    ) {
    }

    private record Snapshot(long createdAtNanos, int size, String title, List<ItemStack> items) {
    }

    public record ProcessedMessage(Component component, List<String> mentionedPlayers) {
    }

    private record FunctionSettings(
        boolean enabled,
        String permission,
        long cooldownMillis,
        boolean notifyPlayer,
        boolean selfMention,
        String pattern,
        List<String> keys
    ) {
        private static FunctionSettings from(Map<?, ?> map) {
            return new FunctionSettings(
                bool(map.get("Enabled"), true),
                string(map.get("Permission"), "none"),
                durationMillis(string(map.get("Cooldown"), "0")),
                bool(map.get("Notify"), true),
                bool(map.get("Self-Mention"), false),
                string(map.get("Pattern"), "@? ?(names)"),
                strings(map.get("Keys"))
            );
        }
    }

    private record Display(
        String text,
        String hover,
        String suggest,
        String command,
        String url,
        String copy
    ) {
        private static Display from(Map<?, ?> map) {
            return new Display(
                string(map.get("text"), ""),
                multiline(map.get("hover")),
                string(map.get("suggest"), ""),
                string(map.get("command"), ""),
                string(map.get("url"), ""),
                string(map.get("copy"), "")
            );
        }
    }

    private record CustomFunction(
        String id,
        String condition,
        int priority,
        Pattern pattern,
        Pattern textFilter,
        FunctionSettings settings,
        Display display
    ) {
    }

    private record CommandRule(
        String source,
        Pattern pattern,
        boolean exact,
        String condition,
        long cooldownMillis
    ) {
    }

    private record Configuration(
        boolean commandControllerEnabled,
        List<CommandRule> commandRules,
        FunctionSettings mention,
        FunctionSettings mentionAll,
        FunctionSettings item,
        FunctionSettings inventory,
        FunctionSettings enderChest,
        List<CustomFunction> customFunctions
    ) {
        private static Configuration empty() {
            FunctionSettings disabled = new FunctionSettings(false, "none", 0, false, false, "", List.of());
            return new Configuration(false, List.of(), disabled, disabled, disabled, disabled, disabled, List.of());
        }

        private static Configuration from(Map<?, ?> root) {
            Map<?, ?> general = map(root.get("General"));
            Map<?, ?> command = map(general.get("Command-Controller"));
            List<CommandRule> rules = new ArrayList<>();
            for (String source : strings(command.get("List"))) {
                String expression = source.substring(0, source.indexOf('{') >= 0 ? source.indexOf('{') : source.length());
                Map<String, String> properties = properties(source);
                try {
                    rules.add(new CommandRule(
                        source,
                        Pattern.compile(expression, Pattern.CASE_INSENSITIVE),
                        Boolean.parseBoolean(properties.getOrDefault("exact", "false")),
                        properties.getOrDefault("condition", ""),
                        secondsMillis(properties.getOrDefault("cooldown", "0"))
                    ));
                } catch (PatternSyntaxException exception) {
                    LOGGER.log(System.Logger.Level.WARNING, "Ignoring invalid command pattern: " + expression);
                }
            }

            List<CustomFunction> custom = new ArrayList<>();
            for (Map.Entry<?, ?> entry : map(root.get("Custom")).entrySet()) {
                Map<?, ?> value = map(entry.getValue());
                try {
                    String filter = string(value.get("text-filter"), "");
                    custom.add(new CustomFunction(
                        String.valueOf(entry.getKey()),
                        string(value.get("condition"), ""),
                        integer(value.get("priority"), 0),
                        Pattern.compile(string(value.get("pattern"), "(?!)"), Pattern.CASE_INSENSITIVE),
                        filter.isBlank() ? null : Pattern.compile(filter, Pattern.CASE_INSENSITIVE),
                        new FunctionSettings(
                            true,
                            string(value.get("permission"), "none"),
                            durationMillis(string(value.get("cooldown"), "0")),
                            false,
                            false,
                            "",
                            List.of()
                        ),
                        Display.from(map(value.get("display")))
                    ));
                } catch (PatternSyntaxException exception) {
                    LOGGER.log(System.Logger.Level.WARNING, "Ignoring invalid custom function " + entry.getKey());
                }
            }
            custom.sort(Comparator.comparingInt(CustomFunction::priority).reversed());
            return new Configuration(
                bool(command.get("Enabled"), bool(command.get("Enable"), true)),
                List.copyOf(rules),
                FunctionSettings.from(map(general.get("Mention"))),
                FunctionSettings.from(map(general.get("Mention-All"))),
                FunctionSettings.from(map(general.get("Item-Show"))),
                FunctionSettings.from(map(general.get("Inventory-Show"))),
                FunctionSettings.from(map(general.get("EnderChest-Show"))),
                List.copyOf(custom)
            );
        }
    }

    private static Map<String, String> properties(String value) {
        Map<String, String> result = new HashMap<>();
        Matcher matcher = PROPERTY.matcher(value);
        while (matcher.find()) {
            result.put(matcher.group(1).trim().toLowerCase(Locale.ROOT), matcher.group(2).trim());
        }
        return result;
    }

    private static long durationMillis(String value) {
        if (value == null || value.isBlank()) return 0;
        Matcher matcher = Pattern.compile("(?i)(\\d+)(ms|s|m|h|d)?").matcher(value.trim());
        if (!matcher.matches()) return 0;
        long number = Long.parseLong(matcher.group(1));
        String unit = matcher.group(2) == null ? "ms" : matcher.group(2).toLowerCase(Locale.ROOT);
        return switch (unit) {
            case "s" -> number * 1000L;
            case "m" -> number * 60_000L;
            case "h" -> number * 3_600_000L;
            case "d" -> number * 86_400_000L;
            default -> number;
        };
    }

    private static long secondsMillis(String value) {
        try {
            return Math.round(Double.parseDouble(value) * 1000.0D);
        } catch (NumberFormatException ignored) {
            return 0;
        }
    }

    private static Map<?, ?> map(Object value) {
        return value instanceof Map<?, ?> map ? map : Map.of();
    }

    private static List<String> strings(Object value) {
        if (value instanceof List<?> list) {
            return list.stream().map(String::valueOf).toList();
        }
        return value == null ? List.of() : List.of(String.valueOf(value));
    }

    private static String string(Object value, String fallback) {
        return value == null || "~".equals(value) ? fallback : String.valueOf(value);
    }

    private static String multiline(Object value) {
        if (value instanceof List<?> list) {
            return String.join("\n", list.stream().map(String::valueOf).toList());
        }
        return string(value, "");
    }

    private static boolean bool(Object value, boolean fallback) {
        return value == null ? fallback : Boolean.parseBoolean(String.valueOf(value));
    }

    private static int integer(Object value, int fallback) {
        if (value instanceof Number number) return number.intValue();
        try {
            return value == null ? fallback : Integer.parseInt(String.valueOf(value));
        } catch (NumberFormatException ignored) {
            return fallback;
        }
    }
}
