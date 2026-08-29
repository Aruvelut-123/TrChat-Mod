plugins {
    id("java-library")
    id("net.neoforged.moddev")
}

version = "${property("mod_version")}+${sc.current.version}"
base {
    archivesName.set("${property("mod_id") as String}-neoforge")
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
val isLegacyObfuscated = mcVersion.startsWith("1.")

// Compute the minecraft version range: [current, next_minor)
fun computeVersionRange(version: String): String {
    val parts = version.split(".")
    if (parts.size == 3) {
        // 1.21.1 -> [1.21.1,1.21.2); 26.1.2 -> [26.1.2,26.2)
        return "[$version,${parts[0]}.${parts[1]}.${parts[2].toInt() + 1})"
    } else if (parts.size == 2) {
        // 26.2 -> [26.2,26.3)
        return "[$version,${parts[0]}.${parts[1].toInt() + 1})"
    }
    return "[$version,$version)"
}

val requiredJava: JavaVersion = when {
    sc.current.parsed >= "26.1" -> JavaVersion.VERSION_25
    else -> JavaVersion.VERSION_21
}

repositories {
    mavenCentral()
    maven("https://maven.fabricmc.net/") { name = "FabricMC" }
    maven("https://maven.neoforged.net/releases/") { name = "NeoForged" }
}

dependencies {
    // Bundled libraries (jarJar) - version ranges use strictly/prefer so NeoForge resolves them.
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

neoForge {
    version = project.property("neo_version") as String

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

    // 26.x ships deobfuscated jars; 1.x needs Mojang mappings handled by moddev internally.
    compileJava {
        dependsOn("stonecutterGenerate")
        options.release.set(requiredJava.majorVersion.toInt())
        options.encoding = "UTF-8"
    }

    test {
        useJUnitPlatform()
    }

    // Expand template metadata (neoforge.mods.toml) with real values for this node.
    processResources {
        dependsOn("stonecutterGenerate")
        val props = mapOf(
            "mod_id" to (project.property("mod_id") as String),
            "mod_name" to (project.property("mod_name") as String),
            "mod_version" to (project.property("mod_version") as String),
            "mod_license" to (project.property("mod_license") as String),
            "minecraft_version_range" to computeVersionRange(mcVersion),
            "neo_version_range" to (project.property("neo_version_range") as String),
            "loader_version_range" to (project.property("loader_version_range") as String),
        )
        inputs.properties(props)
        filesMatching("META-INF/neoforge.mods.toml") { expand(props) }
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
