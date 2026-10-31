// Relay's in-game test driver. Ships only into Relay's own test instance, never to players.
plugins {
    id("fabric-loom") version "1.17.19"
}

version = "${property("mod_version")}"
group = "dev.relay"
base { archivesName.set("relay-testbot") }

dependencies {
    minecraft("com.mojang:minecraft:${property("minecraft_version")}")
    mappings("net.fabricmc:yarn:${property("yarn_mappings")}:v2")
    modImplementation("net.fabricmc:fabric-loader:${property("loader_version")}")
}

tasks.processResources {
    inputs.property("version", project.version)
    filesMatching("fabric.mod.json") { expand("version" to project.version) }
}

java {
    toolchain.languageVersion.set(JavaLanguageVersion.of(21))
}
