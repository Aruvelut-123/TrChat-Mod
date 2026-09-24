pluginManagement {
    repositories {
        mavenCentral()
        gradlePluginPortal()
        maven {
            name = "FabricMC"
            url = uri("https://maven.fabricmc.net/")
        }
        maven {
            name = "NeoForged"
            url = uri("https://maven.neoforged.net/releases/")
        }
    }

    resolutionStrategy {
        eachPlugin {
            when (requested.id.id) {
                // Loom plugin id depends on whether the target Minecraft is obfuscated.
                // Legacy releases (1.21.11 and older) ship obfuscated -> fabric-loom-remap.
                // Year-versioned releases (26.x) ship deobfuscated -> fabric-loom.
                "net.fabricmc.fabric-loom" -> useVersion("1.17.20")
                "net.fabricmc.fabric-loom-remap" -> useModule("net.fabricmc:fabric-loom:1.17.20")
                "net.neoforged.moddev" -> useVersion("2.0.144")
                // Forge (1.20.1) is built through moddevgradle's legacy forge support.
                "net.neoforged.moddev.legacyforge" -> useModule("net.neoforged:moddev-gradle:2.0.144")
                "net.neoforged.moddev.legacyforge.repositories" -> useModule("net.neoforged:moddev-gradle:2.0.144")
            }
        }
    }
}

plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
    id("dev.kikugie.stonecutter") version "0.9.8"
    id("net.neoforged.moddev") version "2.0.146" apply false
}

stonecutter {
    create(rootProject) {
        /**
         * Creates version nodes for multiple loaders.
         *
         * This function will create subprojects named `versions/{project}-{loader}`.
         * Each project has a logical [version], which should match the Minecraft version,
         * whereas [project] is the arbitrary name part of the folder.
         *
         * Each project will also have a separate build script assigned depending on the loader,
         * named `build.{loader}.gradle.kts`.
         */
        fun match(project: String, vararg loaders: String, version: String = project) {
            for (loader in loaders) version("$project-$loader", version).buildscript("build.$loader.gradle.kts")
        }

        // See https://stonecutter.kikugie.dev/wiki/start/#choosing-minecraft-versions
        match("1.20.1", "forge")
        match("1.21.1", "fabric", "neoforge")
        match("1.21.11", "fabric", "neoforge")
        match("26.1.2", "fabric", "neoforge")
        match("26.2", "fabric", "neoforge")
        vcsVersion = "1.21.1-neoforge"
    }
}

rootProject.name = "TrChat-Mod"