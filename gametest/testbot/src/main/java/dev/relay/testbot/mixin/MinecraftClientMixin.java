package dev.relay.testbot.mixin;

import dev.relay.testbot.TestBot;
import net.minecraft.client.MinecraftClient;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(MinecraftClient.class)
abstract class MinecraftClientMixin {
    @Inject(method = "tick", at = @At("TAIL"))
    private void relay$tick(CallbackInfo ci) {
        TestBot.tick((MinecraftClient) (Object) this);
    }
}
