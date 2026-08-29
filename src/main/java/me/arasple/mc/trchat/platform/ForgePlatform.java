//? if forge {
package me.arasple.mc.trchat.platform;

import me.arasple.mc.trchat.TrChatMod;
import net.minecraftforge.fml.ModList;
import net.minecraftforge.fml.loading.FMLEnvironment;
import net.minecraftforge.fml.loading.FMLLoader;
import net.minecraftforge.fml.loading.FMLPaths;

import java.nio.file.Path;

/** Forge 1.20.1 平台实现 */
final class ForgePlatform implements Platform.PlatformImpl {

    @Override
    public Path configDir() {
        return FMLPaths.CONFIGDIR.get();
    }

    @Override
    public boolean isModLoaded(String modId) {
        return ModList.get().isLoaded(modId);
    }

    @Override
    public String modVersion() {
        return ModList.get()
            .getModContainerById(TrChatMod.MOD_ID)
            .map(container -> container.getModInfo().getVersion().toString())
            .orElse("development");
    }

    @Override
    public boolean isDevelopment() {
        return !FMLEnvironment.production && FMLLoader.getDist().isClient();
    }
}
//? }
