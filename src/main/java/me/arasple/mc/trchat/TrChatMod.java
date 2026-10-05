package me.arasple.mc.trchat;

import me.arasple.mc.trchat.config.ConfigMigration;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

//? if neoforge {
import me.arasple.mc.trchat.config.TrChatConfig;
import me.arasple.mc.trchat.util.proxy.transport.NeoForgeProxyTransport;
import net.neoforged.bus.api.IEventBus;
import net.neoforged.fml.ModContainer;
import net.neoforged.fml.common.Mod;
import net.neoforged.fml.config.ModConfig;
import net.neoforged.neoforge.common.NeoForge;
import net.neoforged.neoforge.network.event.RegisterPayloadHandlersEvent;

@Mod(TrChatMod.MOD_ID)
public final class TrChatMod {

    public static final String MOD_ID = "trchat";
    public static final String MOD_NAME = "TrChat Mod";
    public static final Logger LOGGER = LoggerFactory.getLogger("TrChat");

    public TrChatMod(IEventBus modBus, ModContainer container) {
        ConfigMigration.migrateIfNeeded();
        container.registerConfig(ModConfig.Type.COMMON, TrChatConfig.SPEC, "trchat/settings.toml");
        modBus.addListener(RegisterPayloadHandlersEvent.class, NeoForgeProxyTransport::register);
        NeoForge.EVENT_BUS.register(new TrChatServerEvents());
    }
}
//? } else if forge {
import me.arasple.mc.trchat.config.TrChatConfig;
import me.arasple.mc.trchat.util.proxy.transport.ForgeProxyTransport;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.fml.ModLoadingContext;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.config.ModConfig;

@Mod(TrChatMod.MOD_ID)
public final class TrChatMod {

    public static final String MOD_ID = "trchat";
    public static final String MOD_NAME = "TrChat Mod";
    public static final Logger LOGGER = LoggerFactory.getLogger("TrChat");

    public TrChatMod() {
        ConfigMigration.migrateIfNeeded();
        ModLoadingContext.get().registerConfig(ModConfig.Type.COMMON, TrChatConfig.SPEC, "trchat/settings.toml");
        ForgeProxyTransport.register();
        MinecraftForge.EVENT_BUS.register(new TrChatServerEventsForge());
    }
}
//? } else {
import me.arasple.mc.trchat.util.proxy.transport.FabricProxyTransport;
import net.fabricmc.api.DedicatedServerModInitializer;

public final class TrChatMod implements DedicatedServerModInitializer {

    public static final String MOD_ID = "trchat";
    public static final String MOD_NAME = "TrChat Mod";
    public static final Logger LOGGER = LoggerFactory.getLogger("TrChat");

    @Override
    public void onInitializeServer() {
        ConfigMigration.migrateIfNeeded();
        FabricProxyTransport.register();
        new TrChatServerEventsFabric();
    }
}
//? }