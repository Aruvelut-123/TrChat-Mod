package me.arasple.mc.trchat;

import me.arasple.mc.trchat.config.ConfigMigration;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

//? if neoforge {
import me.arasple.mc.trchat.config.TrChatConfig;
import net.neoforged.bus.api.IEventBus;
import net.neoforged.fml.ModContainer;
import net.neoforged.fml.common.Mod;
import net.neoforged.fml.config.ModConfig;
import net.neoforged.neoforge.common.NeoForge;

@Mod(TrChatMod.MOD_ID)
public final class TrChatMod {

    public static final String MOD_ID = "trchat";
    public static final String MOD_NAME = "TrChat Mod";
    public static final Logger LOGGER = LoggerFactory.getLogger("TrChat");

    public TrChatMod(IEventBus modBus, ModContainer container) {
        ConfigMigration.migrateIfNeeded();
        container.registerConfig(ModConfig.Type.COMMON, TrChatConfig.SPEC, "trchat/settings.toml");
        NeoForge.EVENT_BUS.register(new TrChatServerEvents());
    }
}
//? } else {
import net.fabricmc.api.ModInitializer;

public final class TrChatMod implements ModInitializer {

    public static final String MOD_ID = "trchat";
    public static final String MOD_NAME = "TrChat Mod";
    public static final Logger LOGGER = LoggerFactory.getLogger("TrChat");

    @Override
    public void onInitialize() {
        ConfigMigration.migrateIfNeeded();
        new TrChatServerEventsFabric();
    }
}
//? }