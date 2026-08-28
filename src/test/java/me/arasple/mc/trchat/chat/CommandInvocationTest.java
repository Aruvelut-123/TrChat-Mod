package me.arasple.mc.trchat.chat;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;

class CommandInvocationTest {

    @Test
    void parsesVanillaPrivateAliasesForEventRouting() {
        assertEquals(
            new CommandInvocation("tell", "Alex hello there"),
            CommandInvocation.parse("/tell Alex hello there")
        );
        assertEquals(
            new CommandInvocation("msg", "Steve cross-server reply"),
            CommandInvocation.parse("  msg\tSteve cross-server reply")
        );
        assertEquals(new CommandInvocation("tell", ""), CommandInvocation.parse("tell"));
    }
}
