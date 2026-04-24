package taboolib.module.nms;

import taboolib.common.PrimitiveIO;
import taboolib.common.TabooLib;

import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;

/**
 * Compatibility patch for proxy environments where Bukkit is not present.
 *
 * <p>TabooLib's original implementation eagerly touches {@code org.bukkit.Bukkit}
 * in the static initializer, which breaks isolated loading on Velocity.
 * We keep the same API but only resolve Bukkit state when the class exists.</p>
 */
@SuppressWarnings("ALL")
public class MeteorReflection {

    private static final String PAPER_REFLECTION_HOLDER = "io.papermc.paper.pluginremap.reflect.PaperReflectionHolder";

    private static Class<?> paperReflectionHolder;
    private static Method forName;
    private static String minecraftVersion = "UNKNOWN";
    private static boolean isMojangMapping = false;

    static {
        try {
            Class.forName("net.minecraft.core.MappedRegistry");
            isMojangMapping = true;
        } catch (Throwable ignored) {
        }
        try {
            paperReflectionHolder = Class.forName(PAPER_REFLECTION_HOLDER);
            forName = paperReflectionHolder.getDeclaredMethod("forName", String.class, boolean.class, ClassLoader.class);
            forName.setAccessible(true);
        } catch (Throwable ignored) {
        }
        try {
            Class<?> bukkitClass = Class.forName("org.bukkit.Bukkit");
            Method getServer = bukkitClass.getDeclaredMethod("getServer");
            Object server = getServer.invoke(null);
            if (server != null) {
                String obcPackage = server.getClass().getName();
                if (obcPackage.startsWith("org.bukkit.craftbukkit.v1_")) {
                    minecraftVersion = obcPackage.split("\\.")[3];
                }
            }
        } catch (Throwable ignored) {
        }
    }

    public static boolean isMojangMapping() {
        return isMojangMapping;
    }

    static void init() {
        TabooLib.setClassFinder(new TabooLib.ClassFinder() {

            @Override
            public Class<?> getClass(String name) throws ClassNotFoundException {
                return forName(name, true, TabooLib.class.getClassLoader());
            }

            @Override
            public Class<?> getClass(String name, boolean initialize) throws ClassNotFoundException {
                return forName(name, initialize, TabooLib.class.getClassLoader());
            }

            @Override
            public Class<?> getClass(String name, boolean initialize, ClassLoader classLoader) throws ClassNotFoundException {
                return forName(name, initialize, classLoader);
            }
        });
        PrimitiveIO.debug("PaperClassFinder 已生效。");
    }

    public static Class<?> forName(String name, boolean initialize, ClassLoader loader) throws ClassNotFoundException {
        if (forName != null) {
            try {
                return (Class<?>) forName.invoke(null, name, initialize, loader);
            } catch (IllegalAccessException e) {
                throw new RuntimeException(e);
            } catch (InvocationTargetException e) {
                throw new RuntimeException(e);
            }
        } else {
            if (!isMojangMapping) {
                if (!"UNKNOWN".equals(minecraftVersion)
                        && name.startsWith("org.bukkit.craftbukkit")
                        && !name.startsWith("org.bukkit.craftbukkit.v1")) {
                    name = name.replace("org.bukkit.craftbukkit.", "org.bukkit.craftbukkit." + minecraftVersion);
                }
                if (name.startsWith("net.minecraft")) {
                    String translatedName = name.replace(".", "/");
                    name = MinecraftVersion.INSTANCE.getPaperMapping()
                            .getClassMapMojangToSpigot()
                            .getOrDefault(translatedName, translatedName)
                            .replace("/", ".");
                }
            }
            return Class.forName(name, initialize, loader);
        }
    }
}
