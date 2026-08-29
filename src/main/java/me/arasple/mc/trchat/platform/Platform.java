package me.arasple.mc.trchat.platform;

import java.nio.file.Path;

/**
 * 平台抽象层：隔离 NeoForge / Fabric 加载器差异。
 * 实现类通过 Stonecutter 条件注释选择编译。
 */
public final class Platform {

    private static final PlatformImpl IMPL = PlatformImpl.create();

    private Platform() {
    }

    /** 配置文件根目录（config/trchat） */
    public static Path configDir() {
        return IMPL.configDir();
    }

    /** 是否安装了指定 mod */
    public static boolean isModLoaded(String modId) {
        return IMPL.isModLoaded(modId);
    }

    /** 当前 mod 版本号 */
    public static String modVersion() {
        return IMPL.modVersion();
    }

    /** 是否为开发环境 */
    public static boolean isDevelopment() {
        return IMPL.isDevelopment();
    }

    public interface PlatformImpl {
        Path configDir();

        boolean isModLoaded(String modId);

        String modVersion();

        boolean isDevelopment();

        static PlatformImpl create() {
            //? if fabric {
            return new FabricPlatform();
            //? } else {
            return new NeoForgePlatform();
            //? }
        }
    }
}
