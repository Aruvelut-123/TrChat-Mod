plugins {
    id("java-library")
    id("net.neoforged.moddev.legacyforge")
}

version = "${property("mod_version")}+${sc.current.version}"
base {
    archivesName.set("${property("mod_id") as String}-forge")
}

// Stonecutter processes the shared root src/ into this node's generated directory during the
// execution phase (stonecutterGenerate). Point the main source set at the processed output so
// //? if conditions take effect during compilation.
sourceSets.main {
    java.setSrcDirs(listOf(layout.buildDirectory.dir("generated/stonecutter/main/java")))
    resources.setSrcDirs(
        listOf(
            layout.buildDirectory.dir("generated/stonecutter/main/resources"),
            layout.buildDirectory.dir("generated/stonecutter/main/templates"),
        )
    )
}

val mcVersion = sc.current.version

// Compute the minecraft version range: [current, next_minor)
fun computeVersionRange(version: String): String {
    val parts = version.split(".")
    if (parts.size == 3) {
        // 1.20.1 -> [1.20.1,1.20.2)
        return "[$version,${parts[0]}.${parts[1]}.${parts[2].toInt() + 1})"
    } else if (parts.size == 2) {
        return "[$version,${parts[0]}.${parts[1].toInt() + 1})"
    }
    return "[$version,$version)"
}

val requiredJava: JavaVersion = JavaVersion.VERSION_17

repositories {
    mavenCentral()
    maven("https://maven.fabricmc.net/") { name = "FabricMC" }
    maven("https://maven.neoforged.net/releases/") { name = "NeoForged" }
    maven("https://maven.minecraftforge.net/") { name = "Forge" }
}

dependencies {
    // Bundled libraries (jarJar) - version ranges use strictly/prefer so Forge resolves them.
    val bundled = listOf(
        "org.yaml:snakeyaml:2.6",
        "org.xerial:sqlite-jdbc:3.53.2.1",
        "com.mysql:mysql-connector-j:8.4.0",
        "com.google.protobuf:protobuf-java:4.35.1",
        "org.mariadb.jdbc:mariadb-java-client:3.5.10",
        "org.postgresql:postgresql:42.7.5",
    )
    for (lib in bundled) {
        "implementation"(lib)
        "jarJar"(lib) {
            version { strictly("[1.0,)"); prefer(lib.substringAfterLast(':')) }
        }
    }

    testImplementation(platform("org.junit:junit-bom:6.1.3"))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

legacyForge {
    version = project.property("forge_version") as String

    mods {
        register(project.property("mod_id") as String) {
            sourceSet(sourceSets.main.get())
        }
    }

    runs {
        register("server") {
            gameDirectory = file("../../run/")
            server()
        }
    }
}

java {
    withSourcesJar()
    targetCompatibility = requiredJava
    sourceCompatibility = requiredJava

    toolchain {
        languageVersion = JavaLanguageVersion.of(requiredJava.majorVersion)
    }
}

tasks {
    // Compile the Stonecutter-processed shared sources before Minecraft artifacts are built.
    named("createMinecraftArtifacts") {
        dependsOn("stonecutterGenerate")
    }

    // sourcesJar packs the Stonecutter-processed sources; it must run after generation.
    named("sourcesJar") {
        dependsOn("stonecutterGenerate")
    }

    compileJava {
        dependsOn("stonecutterGenerate")
        options.release.set(requiredJava.majorVersion.toInt())
        options.encoding = "UTF-8"
    }

    test {
        useJUnitPlatform()
    }

    // Expand template metadata (mods.toml) with real values for this node.
    processResources {
        dependsOn("stonecutterGenerate")
        val props = mapOf(
            "mod_id" to (project.property("mod_id") as String),
            "mod_name" to (project.property("mod_name") as String),
            "mod_version" to (project.property("mod_version") as String),
            "mod_license" to (project.property("mod_license") as String),
            "minecraft_version_range" to computeVersionRange(mcVersion),
            "forge_version_range" to (project.property("forge_version_range") as String),
            "loader_version_range" to (project.property("forge_version_range") as String),
        )
        inputs.properties(props)
        filesMatching("META-INF/mods.toml") { expand(props) }
        exclude("fabric.mod.json")
        exclude("META-INF/neoforge.mods.toml")
    }

    jar {
        manifest {
            attributes(
                "Title" to (project.property("mod_name") as String),
                "Version" to (project.property("mod_version") as String),
                "Vendor" to "Baymaxawa",
                "Specification-Vendor" to "Baymaxawa",
            )
        }
    }
}