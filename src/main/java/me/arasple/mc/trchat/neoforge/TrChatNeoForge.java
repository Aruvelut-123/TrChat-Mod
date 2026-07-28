package me.arasple.mc.trchat.neoforge;

import me.arasple.mc.trchat.neoforge.config.TrChatConfig;
import net.neoforged.bus.api.IEventBus;
import net.neoforged.fml.ModContainer;
import net.neoforged.fml.common.Mod;
import net.neoforged.fml.config.ModConfig;
import net.neoforged.neoforge.common.NeoForge;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

@Mod(TrChatNeoForge.MOD_ID)
public final class TrChatNeoForge {

    public static final String MOD_ID = "trchat_neoforge";
    public static final String MOD_NAME = "TrChat NeoForge";
    public static final Logger LOGGER = LoggerFactory.getLogger(MOD_NAME);

    public TrChatNeoForge(IEventBus modBus, ModContainer container) {
        container.registerConfig(ModConfig.Type.COMMON, TrChatConfig.SPEC, "trchat-neoforge/settings.toml");
        NeoForge.EVENT_BUS.register(new TrChatServerEvents());
    }
}
