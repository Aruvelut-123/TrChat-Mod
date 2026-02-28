dependencies {
    compileOnly(project(":project:common"))
    compileOnly(project(":project:module-chat"))
    compileOnly("net.kyori:adventure-platform-bukkit:4.4.1")
    compileOnly("net.kyori:adventure-text-serializer-plain:4.26.1")
    implementation("net.kyori:adventure-text-minimessage:4.26.1")
    // paper native
    compileOnly(fileTree(rootDir.resolve("libs")))
}

taboolib { subproject = true }