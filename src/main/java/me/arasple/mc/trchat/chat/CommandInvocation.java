package me.arasple.mc.trchat.chat;

public record CommandInvocation(String alias, String arguments) {

    public static CommandInvocation parse(String commandLine) {
        String normalized = commandLine == null ? "" : commandLine.stripLeading();
        if (normalized.startsWith("/")) {
            normalized = normalized.substring(1).stripLeading();
        }
        int separator = -1;
        for (int index = 0; index < normalized.length(); index++) {
            if (Character.isWhitespace(normalized.charAt(index))) {
                separator = index;
                break;
            }
        }
        return separator < 0
            ? new CommandInvocation(normalized, "")
            : new CommandInvocation(normalized.substring(0, separator), normalized.substring(separator).stripLeading());
    }
}
