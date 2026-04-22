dependencies {
    compileOnly(project(":project:common"))
    compileOnly(project(":project:module-chat"))
    compileOnly(project(":project:module-nms"))
    compileOnly("ink.ptms.core:v260100:260100")
    compileOnly("com.velocitypowered:velocity-brigadier:1.0.0-SNAPSHOT")
}

taboolib { subproject = true }