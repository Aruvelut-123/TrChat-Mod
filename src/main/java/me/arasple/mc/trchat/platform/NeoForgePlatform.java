//? if neoforge {
package me.arasple.mc.trchat.platform;

import me.arasple.mc.trchat.TrChatMod;
import net.neoforged.fml.ModList;
import net.neoforged.fml.loading.FMLEnvironment;
import net.neoforged.fml.loading.FMLLoader;
import net.neoforged.fml.loading.FMLPaths;

import java.nio.file.Path;

/** NeoForge 平台实现 */
final class NeoForgePlatform implements Platform.PlatformImpl {

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
        //? if >=1.21.11 {
        return !FMLEnvironment.isProduction() && FMLEnvironment.getDist().isClient();
        //? } else {
        return !FMLEnvironment.production && FMLLoader.getDist().isClient();
        //? }
    }
}
//? }
