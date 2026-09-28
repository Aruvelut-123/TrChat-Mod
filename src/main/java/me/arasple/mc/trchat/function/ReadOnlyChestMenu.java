package me.arasple.mc.trchat.function;

import net.minecraft.world.Container;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.inventory.ChestMenu;
//? if >=26.1 {
import net.minecraft.world.inventory.ContainerInput;
//? } else {
import net.minecraft.world.inventory.ClickType;
//? }
import net.minecraft.world.inventory.MenuType;
import net.minecraft.world.inventory.Slot;
import net.minecraft.world.item.ItemStack;

final class ReadOnlyChestMenu extends ChestMenu {

    private ReadOnlyChestMenu(
        MenuType<?> type,
        int containerId,
        Inventory inventory,
        Container container,
        int rows
    ) {
        super(type, containerId, inventory, container, rows);
    }

    static ChestMenu create(int containerId, Inventory inventory, Container container, int size) {
        if (size == 54) {
            return new ReadOnlyChestMenu(MenuType.GENERIC_9x6, containerId, inventory, container, 6);
        }
        return new ReadOnlyChestMenu(MenuType.GENERIC_9x3, containerId, inventory, container, 3);
    }

    @Override
    //? if >=26.1 {
    public void clicked(int slotId, int button, ContainerInput clickType, Player player) {
        //? } else {
    public void clicked(int slotId, int button, ClickType clickType, Player player) {
        //? }
        broadcastFullState();
    }

    @Override
    public ItemStack quickMoveStack(Player player, int index) {
        return ItemStack.EMPTY;
    }

    @Override
    public boolean canDragTo(Slot slot) {
        return false;
    }

    @Override
    public boolean canTakeItemForPickAll(ItemStack stack, Slot slot) {
        return false;
    }
}
