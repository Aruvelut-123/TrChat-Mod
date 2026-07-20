repositories {
    maven("https://maven.devs.beer/")
    maven("https://nexus.scarsz.me/content/groups/public/")
    maven("https://repo.oraxen.com/releases")
    maven("https://repo.sayandev.org/snapshots")
//    maven("https://repo.nexomc.com/releases")
}

dependencies {
    compileOnly(project(":project:common"))
    compileOnly(project(":project:module-chat"))
    compileOnly("ink.ptms.core:v12111:12111:universal")

    compileOnly("com.discordsrv:discordsrv:1.26.0") { isTransitive = false }
    compileOnly("com.willfp:eco:6.35.1") { isTransitive = false }

    compileOnly("dev.lone:api-itemsadder:4.0.10") { isTransitive = false }
//    compileOnly("io.th0rgal:oraxen:1.170.0") { isTransitive = false }
//    compileOnly("com.nexomc:nexo:0.7.0")
    compileOnly("xyz.xenondevs.nova:nova-api:0.12.13") { isTransitive = false }

}

taboolib { subproject = true }
