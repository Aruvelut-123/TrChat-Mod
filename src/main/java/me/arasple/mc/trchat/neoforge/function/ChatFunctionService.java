package me.arasple.mc.trchat.neoforge.function;

import me.arasple.mc.trchat.neoforge.channel.ConditionEvaluator;
import me.arasple.mc.trchat.neoforge.chat.LegacyText;
import me.arasple.mc.trchat.neoforge.lang.LanguageService;
import me.arasple.mc.trchat.neoforge.permission.TrChatPermissions;
import net.minecraft.ChatFormatting;
import net.minecraft.network.chat.ClickEvent;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.HoverEvent;
import net.minecraft.network.chat.MutableComponent;
import net.minecraft.network.chat.Style;
import net.minecraft.network.protocol.game.ClientboundSetSubtitleTextPacket;
import net.minecraft.network.protocol.game.ClientboundSetTitleTextPacket;
import net.minecraft.network.protocol.game.ClientboundSetTitlesAnimationPacket;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.sounds.SoundEvents;
import net.minecraft.sounds.SoundSource;
import net.minecraft.world.SimpleContainer;
import net.minecraft.world.SimpleMenuProvider;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.inventory.ChestMenu;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.core.component.DataComponents;
import net.minecraft.world.item.component.ItemContainerContents;
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
    private static final Pattern KETHER_COMMAND = Pattern.compile(
        "(?i)^command\\s+['\"](.+)['\"]\\s+as\\s+(console|player)$"
    );

    private final MinecraftServer server;
    private final Path file;
    private final LanguageService languages;
    private final Map<String, Long> cooldowns = new HashMap<>();
    private final Map<String, Snapshot> snapshots = new LinkedHashMap<>();
    private volatile Configuration configuration = Configuration.empty();

    public ChatFunctionService(MinecraftServer server, LanguageService languages) {
        this(server, languages, FMLPaths.CONFIGDIR.get().resolve("trchat-neoforge").resolve("function.yml"));
    }

    ChatFunctionService(MinecraftServer server, LanguageService languages, Path file) {
        this.server = server;
        this.languages = languages;
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
        Set<String> cooldownGranted = new java.util.HashSet<>();
        Set<String> actionsRun = new java.util.HashSet<>();
        cursor = 0;
        for (Token token : accepted) {
            if (token.start() < cursor) continue;
            output.append(Component.literal(message.substring(cursor, token.start())));
            if (!canUse(sender, token, cooldownGranted)) {
                output.append(Component.literal(message.substring(token.start(), token.end())));
            } else {
                String actionKey = token.custom() == null ? token.kind().name() : "custom:" + token.custom().id();
                if (actionsRun.add(actionKey)) {
                    runActions(sender, token.settings().actions(), message, token.argument());
                }
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
            viewer.sendSystemMessage(languages.component(viewer, "Function-Snapshot-Expired"));
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
        CommandController.Rule rule = matchingRule(commandLine, config);
        if (rule == null) {
            return true;
        }
        if (!rule.condition().isBlank() && !ConditionEvaluator.test(rule.condition(), player)) {
            player.sendSystemMessage(languages.component(player, "Command-Controller-Deny"));
            return false;
        }
        if (rule.cooldownMillis() > 0
            && !TrChatPermissions.check(player, "trchat.bypass.cmdcooldown")
            && !cooldown(player, "command:" + rule.source(), rule.cooldownMillis())) {
            player.sendSystemMessage(languages.component(player, "Command-Controller-Cooldown"));
            return false;
        }
        return true;
    }

    public boolean isCommandManaged(String commandLine) {
        Configuration config = configuration;
        return config.commandControllerEnabled() && matchingRule(commandLine, config) != null;
    }

    public boolean commandControllerEnabled() {
        return configuration.commandControllerEnabled();
    }

    public int commandRuleCount() {
        return configuration.commandRules().size();
    }

    private static CommandController.Rule matchingRule(String commandLine, Configuration config) {
        return CommandController.matching(commandLine, config.commandRules());
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

    private boolean canUse(ServerPlayer sender, Token token, Set<String> cooldownGranted) {
        FunctionSettings settings = token.settings();
        if (!settings.permission().equalsIgnoreCase("none")
            && !TrChatPermissions.check(sender, settings.permission())) {
            return false;
        }
        if (settings.cooldownMillis() <= 0) return true;
        String key = token.custom() == null ? token.kind().name() : "custom:" + token.custom().id();
        return cooldownGranted.contains(key)
            || cooldown(sender, key, settings.cooldownMillis()) && cooldownGranted.add(key);
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
            case ITEM -> item(sender, token.argument(), token.settings());
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
            notifyMention(target, sender.getGameProfile().getName());
        }
        return Component.literal("@" + name)
            .withStyle(ChatFormatting.AQUA)
            .withStyle(style -> style.withHoverEvent(new HoverEvent(
                HoverEvent.Action.SHOW_TEXT,
                languages.component(sender, "Function-Mention-Hover", sender.getGameProfile().getName(), name)
            )));
    }

    private Component mentionAll(ServerPlayer sender, List<String> mentioned) {
        if (configuration.mentionAll().notifyPlayer()) {
            for (ServerPlayer target : server.getPlayerList().getPlayers()) {
                if (!target.getUUID().equals(sender.getUUID())) {
                    mentioned.add(target.getGameProfile().getName());
                    notifyMention(target, sender.getGameProfile().getName());
                }
            }
        }
        return Component.literal("@所有人")
            .withStyle(ChatFormatting.GOLD, ChatFormatting.BOLD)
            .withStyle(style -> style.withHoverEvent(new HoverEvent(
                HoverEvent.Action.SHOW_TEXT,
                languages.component(sender, "Function-Mention-All-Hover", sender.getGameProfile().getName())
            )));
    }

    private void notifyMention(ServerPlayer target, String senderName) {
        target.playNotifySound(SoundEvents.ANVIL_LAND, SoundSource.PLAYERS, 1.0F, 2.0F);
        target.displayClientMessage(
            languages.component(target, "Function-Mention-Notify", senderName),
            true
        );
        target.connection.send(new ClientboundSetTitlesAnimationPacket(10, 50, 10));
        target.connection.send(new ClientboundSetTitleTextPacket(
            languages.component(target, "Function-Mention-Title", senderName)
        ));
        target.connection.send(new ClientboundSetSubtitleTextPacket(
            languages.component(target, "Function-Mention-Subtitle", senderName)
        ));
    }

    private Component item(ServerPlayer sender, String slotArgument, FunctionSettings settings) {
        int slot = slotArgument.isBlank() ? sender.getInventory().selected : Integer.parseInt(slotArgument) - 1;
        ItemStack stack = sender.getInventory().getItem(slot);
        if (stack.isEmpty()) {
            return languages.component(sender, "Function-Item-Air").copy().withStyle(ChatFormatting.GRAY);
        }
        ItemStack hoverStack = settings.compatible() ? new ItemStack(Items.STONE, stack.getCount()) : stack;
        Component itemName = settings.originName()
            ? Component.translatable(stack.getDescriptionId())
            : stack.getHoverName();
        MutableComponent component = Component.literal("[")
            .append(itemName)
            .append(Component.literal(" x" + stack.getCount() + "]"));
        MutableComponent rendered = component.withStyle(style -> style
            .withColor(ChatFormatting.AQUA)
            .withHoverEvent(new HoverEvent(HoverEvent.Action.SHOW_ITEM, new HoverEvent.ItemStackInfo(hoverStack))));
        if (settings.ui()) {
            String snapshot = createItemSnapshot(sender, stack);
            rendered.withStyle(style -> style.withClickEvent(
                new ClickEvent(ClickEvent.Action.RUN_COMMAND, "/trchat view " + snapshot)
            ));
        }
        return rendered;
    }

    private Component inventory(ServerPlayer sender, boolean enderChest) {
        String snapshotId = createSnapshot(sender, enderChest);
        String labelKey = enderChest ? "Function-EnderChest-Format" : "Function-Inventory-Format";
        String hoverKey = enderChest ? "Function-EnderChest-Hover" : "Function-Inventory-Hover";
        return languages.component(sender, labelKey, sender.getGameProfile().getName()).copy()
            .withStyle(ChatFormatting.AQUA)
            .withStyle(style -> style
                .withHoverEvent(new HoverEvent(
                    HoverEvent.Action.SHOW_TEXT,
                    languages.component(sender, hoverKey)
                ))
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
            title = languages.text(player, "Function-EnderChest-Title", player.getGameProfile().getName());
            for (int index = 0; index < 27; index++) {
                items.add(player.getEnderChestInventory().getItem(index).copy());
            }
        } else {
            size = 54;
            title = languages.text(player, "Function-Inventory-Title", player.getGameProfile().getName());
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

    private String createItemSnapshot(ServerPlayer player, ItemStack stack) {
        expireSnapshots();
        String id = UUID.randomUUID().toString().replace("-", "").substring(0, 12);
        List<ItemStack> items = new ArrayList<>();
        ItemContainerContents contents = stack.get(DataComponents.CONTAINER);
        if (contents != null) {
            for (ItemStack nested : contents.nonEmptyItems()) {
                if (items.size() >= 27) break;
                items.add(nested.copy());
            }
        }
        if (items.isEmpty()) {
            for (int index = 0; index < 13; index++) items.add(ItemStack.EMPTY);
            items.add(stack.copy());
        }
        snapshots.put(id, new Snapshot(
            System.nanoTime(), 27,
            languages.text(
                player, "Function-Item-Title",
                player.getGameProfile().getName(), stack.getHoverName().getString()
            ),
            List.copyOf(items)
        ));
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

    private void runActions(ServerPlayer sender, List<String> actions, String message, String argument) {
        for (String configured : actions) {
            String action = actionVariables(configured, sender, message, argument).trim();
            if (action.isEmpty()) continue;
            try {
                Matcher ketherCommand = KETHER_COMMAND.matcher(action);
                if (ketherCommand.matches()) {
                    executeCommand(sender, ketherCommand.group(2), ketherCommand.group(1));
                } else if (action.regionMatches(true, 0, "console:", 0, 8)) {
                    executeCommand(sender, "console", action.substring(8));
                } else if (action.regionMatches(true, 0, "[console]", 0, 9)) {
                    executeCommand(sender, "console", action.substring(9));
                } else if (action.regionMatches(true, 0, "player:", 0, 7)) {
                    executeCommand(sender, "player", action.substring(7));
                } else if (action.regionMatches(true, 0, "[player]", 0, 8)) {
                    executeCommand(sender, "player", action.substring(8));
                } else if (action.regionMatches(true, 0, "message:", 0, 8)) {
                    sender.sendSystemMessage(LegacyText.parse(action.substring(8).trim()));
                } else if (action.toLowerCase(Locale.ROOT).startsWith("tell ")) {
                    sender.sendSystemMessage(LegacyText.parse(unquote(action.substring(5).trim())));
                } else if (action.regionMatches(true, 0, "sound:", 0, 6)) {
                    String sound = action.substring(6).trim();
                    executeCommand(sender, "console", "playsound " + sound + " master "
                        + sender.getGameProfile().getName() + " ~ ~ ~");
                } else {
                    LOGGER.log(System.Logger.Level.WARNING, "Unsupported function action: " + configured);
                }
            } catch (RuntimeException exception) {
                LOGGER.log(System.Logger.Level.WARNING, "Unable to execute function action: " + configured);
            }
        }
    }

    private void executeCommand(ServerPlayer sender, String actor, String command) {
        String normalized = unquote(command.trim());
        if (normalized.startsWith("/")) normalized = normalized.substring(1);
        if (normalized.isBlank()) return;
        server.getCommands().performPrefixedCommand(
            actor.equalsIgnoreCase("player")
                ? sender.createCommandSourceStack()
                : server.createCommandSourceStack(),
            normalized
        );
    }

    private static String actionVariables(
        String action,
        ServerPlayer sender,
        String message,
        String argument
    ) {
        return action
            .replace("{player}", sender.getGameProfile().getName())
            .replace("%player_name%", sender.getGameProfile().getName())
            .replace("{message}", message)
            .replace("{0}", argument);
    }

    private static String unquote(String value) {
        if (value.length() >= 2
            && (value.startsWith("\"") && value.endsWith("\"")
            || value.startsWith("'") && value.endsWith("'"))) {
            return value.substring(1, value.length() - 1);
        }
        return value;
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
        List<String> keys,
        List<String> actions,
        boolean originName,
        boolean compatible,
        boolean ui
    ) {
        private static FunctionSettings from(Map<?, ?> map) {
            List<String> actions = new ArrayList<>(strings(map.get("Action")));
            actions.addAll(strings(map.get("Actions")));
            return new FunctionSettings(
                bool(map.get("Enabled"), true),
                string(map.get("Permission"), "none"),
                durationMillis(string(map.get("Cooldown"), "0")),
                bool(map.get("Notify"), true),
                bool(map.get("Self-Mention"), false),
                string(map.get("Pattern"), "@? ?(names)"),
                strings(map.get("Keys")),
                List.copyOf(actions),
                bool(map.get("Origin-Name"), false),
                bool(map.get("Compatible"), false),
                bool(map.get("UI"), false)
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

    private record Configuration(
        boolean commandControllerEnabled,
        List<CommandController.Rule> commandRules,
        FunctionSettings mention,
        FunctionSettings mentionAll,
        FunctionSettings item,
        FunctionSettings inventory,
        FunctionSettings enderChest,
        List<CustomFunction> customFunctions
    ) {
        private static Configuration empty() {
            FunctionSettings disabled = new FunctionSettings(
                false, "none", 0, false, false, "", List.of(), List.of(), false, false, false
            );
            return new Configuration(false, List.of(), disabled, disabled, disabled, disabled, disabled, List.of());
        }

        private static Configuration from(Map<?, ?> root) {
            Map<?, ?> general = map(root.get("General"));
            Map<?, ?> command = map(general.get("Command-Controller"));
            CommandController.Configuration controller = CommandController.from(command);

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
                            List.of(),
                            actions(value),
                            false,
                            false,
                            false
                        ),
                        Display.from(map(value.get("display")))
                    ));
                } catch (PatternSyntaxException exception) {
                    LOGGER.log(System.Logger.Level.WARNING, "Ignoring invalid custom function " + entry.getKey());
                }
            }
            custom.sort(Comparator.comparingInt(CustomFunction::priority).reversed());
            return new Configuration(
                controller.enabled(),
                controller.rules(),
                FunctionSettings.from(map(general.get("Mention"))),
                FunctionSettings.from(map(general.get("Mention-All"))),
                FunctionSettings.from(map(general.get("Item-Show"))),
                FunctionSettings.from(map(general.get("Inventory-Show"))),
                FunctionSettings.from(map(general.get("EnderChest-Show"))),
                List.copyOf(custom)
            );
        }
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

    private static Map<?, ?> map(Object value) {
        return value instanceof Map<?, ?> map ? map : Map.of();
    }

    private static List<String> strings(Object value) {
        if (value instanceof List<?> list) {
            return list.stream().map(String::valueOf).toList();
        }
        return value == null ? List.of() : List.of(String.valueOf(value));
    }

    private static List<String> actions(Map<?, ?> map) {
        List<String> actions = new ArrayList<>(strings(map.get("action")));
        actions.addAll(strings(map.get("actions")));
        actions.addAll(strings(map.get("Action")));
        actions.addAll(strings(map.get("Actions")));
        return List.copyOf(actions);
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
