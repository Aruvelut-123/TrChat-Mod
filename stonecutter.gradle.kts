plugins {
    id("dev.kikugie.stonecutter")
}

stonecutter active "1.21.1-neoforge"

// See https://stonecutter.kikugie.dev/wiki/config/params
stonecutter parameters {
    val loader = current.project.substringAfterLast("-")

    // Adds constants to Stonecutter comments (i.e. for `//? if fabric {...`)
    constants {
        match(loader, "fabric", "neoforge", "forge")
    }

    dependencies["minecraft"] = current.version

    // Minecraft API renames across versions are handled here so the shared source stays loader-agnostic.
    replacements {
        // 1.21.11+ renamed ResourceLocation to Identifier (net.minecraft.resources)
        string(current.parsed >= "1.21.11") {
            replace("ResourceLocation", "Identifier")
            // 1.21.11+ turned GameProfile into a record: getName() -> name()
            replace("getGameProfile().getName()", "getGameProfile().name()")
        }
    }
}