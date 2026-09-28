package me.arasple.mc.trchat.placeholder;

import net.minecraft.server.level.ServerPlayer;

import java.util.HashMap;
import java.util.Map;
import java.util.UUID;

public final class PlayerStatsTracker {

    private final Map<UUID, DamageRecord> damage = new HashMap<>();

    public void recordDamage(ServerPlayer player, float amount) {
        damage.put(player.getUUID(), new DamageRecord(amount, player.tickCount));
    }

    public float lastDamage(ServerPlayer player) {
        DamageRecord record = damage.get(player.getUUID());
        return record == null ? 0.0F : record.amount();
    }

    public int noDamageTicks(ServerPlayer player) {
        DamageRecord record = damage.get(player.getUUID());
        return record == null ? 0 : Math.max(0, 20 - (player.tickCount - record.entityTick()));
    }

    public void remove(UUID playerId) {
        damage.remove(playerId);
    }

    private record DamageRecord(float amount, int entityTick) {
    }
}
