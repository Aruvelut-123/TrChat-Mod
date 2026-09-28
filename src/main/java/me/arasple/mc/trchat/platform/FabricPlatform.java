//? if fabric {
package me.arasple.mc.trchat.platform;

import me.arasple.mc.trchat.TrChatMod;
import net.fabricmc.loader.api.FabricLoader;
import net.fabricmc.loader.api.ModContainer;

import java.nio.file.Path;
import java.util.Optional;

/** Fabric platform implementation */
final class FabricPlatform implements Platform.PlatformImpl {

    private static final FabricLoader LOADER = FabricLoader.getInstance();

    @Override
    public Path configDir() {
        return LOADER.getConfigDir();
    }

    @Override
    public boolean isModLoaded(String modId) {
        return LOADER.isModLoaded(modId);
    }

    @Override
    public String modVersion() {
        Optional<ModContainer> container = LOADER.getModContainer(TrChatMod.MOD_ID);
        return container
            .map(c -> c.getMetadata().getVersion().getFriendlyString())
            .orElse("development");
    }

    @Override
    public boolean isDevelopment() {
        return LOADER.isDevelopmentEnvironment();
    }
}
//? }