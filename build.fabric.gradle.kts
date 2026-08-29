plugins {
    id("java-library")
    id("net.fabricmc.fabric-loom-remap") apply false
    id("net.fabricmc.fabric-loom") apply false
}

version = "${property("mod_version")}+${sc.current.version}"
base {
    archivesName.set("${property("mod_id") as String}-fabric")
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
// 26.x is NeoForge year-version scheme; 1.x is standard Minecraft versioning.
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

// Loom plugin id depends on whether the target Minecraft is obfuscated.
// The plugin version is supplied via settings.gradle.kts resolutionStrategy.
if (isLegacyObfuscated) {
    apply(plugin = "net.fabricmc.fabric-loom-remap")
} else {
    apply(plugin = "net.fabricmc.fabric-loom")
}

val requiredJava: JavaVersion = when {
    sc.current.parsed >= "26.1" -> JavaVersion.VERSION_25
    else -> JavaVersion.VERSION_21
}

val fabricLoaderVersion = property("fabric_loader_version") as String
val fabricApiVersion = property("fabric_api_version") as String

repositories {
    mavenCentral()
    maven("https://maven.fabricmc.net/") { name = "FabricMC" }
    maven("https://maven.neoforged.net/releases/") { name = "NeoForged" }
}

// Dynamically-applied Loom still registers its extension; use the typed API for runs/config.
val loomExt = the<net.fabricmc.loom.api.LoomGradleExtensionAPI>()

dependencies {
    add("minecraft", "com.mojang:minecraft:$mcVersion")
    if (isLegacyObfuscated) {
        add("mappings", loomExt.officialMojangMappings())
        add("modImplementation", "net.fabricmc:fabric-loader:$fabricLoaderVersion")
        add("modImplementation", "net.fabricmc.fabric-api:fabric-api:$fabricApiVersion")
    } else {
        implementation("net.fabricmc:fabric-loader:$fabricLoaderVersion")
        implementation("net.fabricmc.fabric-api:fabric-api:$fabricApiVersion")
    }

    // Bundled libraries.
    implementation("org.yaml:snakeyaml:2.6")
    implementation("org.xerial:sqlite-jdbc:3.53.2.1")
    implementation("com.mysql:mysql-connector-j:8.4.0")
    implementation("com.google.protobuf:protobuf-java:4.35.1")
    implementation("org.mariadb.jdbc:mariadb-java-client:3.5.10")
    implementation("org.postgresql:postgresql:42.7.5")

    testImplementation(platform("org.junit:junit-bom:6.1.3"))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

loomExt.runs.configureEach {
    if (name == "server") {
        // Match the shared run directory layout used across versions.
        runDir(rootProject.file("../../run/").absolutePath)
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
    // Compile the Stonecutter-processed shared sources before Loom sets up the Minecraft dependencies.
    withType<JavaCompile>().configureEach {
        dependsOn("stonecutterGenerate")
    }

    compileJava {
        options.release.set(requiredJava.majorVersion.toInt())
        options.encoding = "UTF-8"
    }

    test {
        useJUnitPlatform()
    }

    processResources {
        dependsOn("stonecutterGenerate")
        val props = mapOf(
            "mod_id" to (project.property("mod_id") as String),
            "mod_name" to (project.property("mod_name") as String),
            "mod_version" to (project.property("mod_version") as String),
            "mod_license" to (project.property("mod_license") as String),
            "minecraft_version_range" to computeVersionRange(mcVersion),
        )
        inputs.properties(props)
        filesMatching("fabric.mod.json") { expand(props) }
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
