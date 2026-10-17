<!-- Fixture standing in for the command-code CLI's generated reference.
     Covers the three cases the parser has to tell apart: an `off` that
     becomes a toggle, an em dash meaning no thinking setting, and the
     Anthropic section, which is the Messages wire. -->

# Command Code Models

## Open Source

| Id (use EXACTLY this) | Name | Context | Efforts | $/1M in/out | Min plan | Best for |
|---|---|---|---|---|---|---|
| `vendor/chat-thinker` | Chat Thinker | 1M | off, high, max | $1/$3 | Go | thinking |
| `vendor/chat-plain` | Chat Plain | 200K | — | $1/$3 | Go | plain |

## Anthropic

| Id (use EXACTLY this) | Name | Context | Efforts | $/1M in/out | Min plan | Best for |
|---|---|---|---|---|---|---|
| `vendor/claude-1` | Claude 1 | 1M | low, high | $3/$15 | Go | messages wire |