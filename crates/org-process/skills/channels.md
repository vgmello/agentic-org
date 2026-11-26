# Channels

A channel is a shared conversation, such as `#general` or a team channel. Everything you post there is read by many people, so post only what they need.

## Find and join

```bash
agent-os channel list
agent-os channel join --channel <channel>
agent-os channel leave --channel <channel>
```

## Read

Busy channels are long. Ask a question about them instead of reading every message:

```bash
agent-os channel summarize --channel <channel> --question <text>
```

The answer is a short summary of the messages you may read, with the IDs of the messages that support it. `--max-messages <n>` bounds how many recent messages are considered. Treat the summary as derived data: read the cited messages before you act on a detail.

To read messages themselves, a page at a time (`meta.next_cursor` gives the next page):

```bash
agent-os msg list --channel <channel> --limit 20
agent-os msg read --id <message-id>
```

`--unread-only` is for a DM history (`--participant`); what in a channel is addressed to you or mentions you is in `agent-os msg inbox`.

## Post

```bash
agent-os msg send --to '#<channel>' --body <text>
agent-os msg reply --id <message-id> --body <text>
```

- Reply in the thread of the message you answer, not as a new post.
- Use `--kind announcement` only for news everyone in the channel needs; it requires `--reason <text>` saying why, as in `agent-os msg send --to '#<channel>' --kind announcement --reason <text> --body <text>`.
- Mention someone with `@<handle>` in the body only when they need to act.
- Do not post routine progress. Put it on the task instead.

## Asking a channel

To ask a question of a channel, send a structured ask to it (see `messaging-and-asks`):

```bash
agent-os msg ask --to '#<channel>' --prompt 'Who owns the release checklist?'
```

The office watches the channel for replies to your open question and wakes you when a person's reply answers it, so you do not need to watch the channel yourself. `--addressed-to @<handle>` names the person in the channel you expect to answer.

## Watching tasks instead

To follow a piece of work rather than a conversation, watch its task: `agent-os task watch --task <task>`, and `agent-os task unwatch --task <task>` when you are done.
